#include <furi.h>
#include <gui/gui.h>
#include <input/input.h>
#include <storage/storage.h>
#include <furi_hal_bt.h>
#include <stdio.h>
#include <stdlib.h>
#include "status_view.h"
#include "node_cache.h"
#include "glimdock_ble.h"
#include "flipper_ble_protocol.h"

typedef struct {
    FuriMutex* model_mutex;
    FuriMessageQueue* input_queue;
    ViewPort* viewport;
    Bt* bt;
    GlimdockBle* profile;
    GdView view;
    GdNodeCache cache;
    bool selection_dirty;
    bool startup_complete;
    bool connected;
    bool exit_requested;
    bool storage_error;
    char error[32];
} Glimdock;

static void draw(Canvas* canvas, void* context) {
    Glimdock* app = context;
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    uint32_t now = furi_get_tick();
    app->view.available = gd_node_cache_project(&app->cache, &app->view.status, now);
    /* Projection already ages each cached page from its own receipt time. */
    app->view.received_ms = now;
    canvas_clear(canvas);
    canvas_set_font(canvas, FontPrimary);
    if(app->error[0]) {
        canvas_draw_str(canvas, 2, 10, app->storage_error ? "App data unavailable" : "Bluetooth unavailable");
        canvas_set_font(canvas, FontSecondary);
        canvas_draw_str(canvas, 2, 29, app->error);
        canvas_draw_str(canvas, 2, 61, "Back: exit");
    } else if(!app->view.available) {
        canvas_draw_str(canvas, 2, 10, "Glimdock Bluetooth");
        canvas_set_font(canvas, FontSecondary);
        if(!app->startup_complete) {
            canvas_draw_str(canvas, 2, 29, "Starting Bluetooth...");
        } else if(app->connected) {
            canvas_draw_str(canvas, 2, 25, app->cache.catalog_sequence_seen ?
                            "No monitored nodes." : "Syncing node summaries...");
            canvas_draw_str(canvas, 2, 36, "Up/Down: node");
            canvas_draw_str(canvas, 2, 47, "Left/Right: page");
        } else {
            canvas_draw_str(canvas, 2, 25, "Ready to connect.");
            canvas_draw_str(canvas, 2, 36, "Tap Pair on Glimdock.");
            canvas_draw_str(canvas, 2, 47, "Enter Flipper's code there.");
        }
        canvas_draw_str(canvas, 2, 61, "Back: exit");
    } else {
        const GdStatusPage* page = &app->view.status.pages[app->view.selected];
        char header[24];
        snprintf(header, sizeof(header), "%u/4 %s", app->view.selected + 1u, page->title);
        canvas_draw_str(canvas, 2, 9, header);
        canvas_draw_line(canvas, 0, 12, 127, 12);
        canvas_set_font(canvas, FontSecondary);
        for(size_t row = 0; row < GD_STATUS_ROWS; ++row)
            canvas_draw_str(canvas, 2, 23 + (uint8_t)row * 9, page->rows[row]);
        char footer[32];
        char age[12];
        bool loading = gd_node_cache_selected_loading(&app->cache);
        const char* state = !app->connected ? "DISCONNECTED" :
                            loading && !(page->flags & GD_STATUS_VALID) ? "LOADING" :
                            gd_view_stream_stale(&app->view, now) ? "NO UPDATES" :
                            gd_view_page_stale(&app->view, now) ? "STALE" :
                            loading ? (app->view.selected == 0 ? "SUMMARY" : "CACHED") : "LIVE";
        const char* demo = page->flags & GD_STATUS_DEMO ? " DEMO" : "";
        if(page->age_seconds == GD_STATUS_UNKNOWN_AGE)
            strlcpy(age, "?", sizeof(age));
        else
            snprintf(age, sizeof(age), "%lus",
                     (unsigned long)page->age_seconds + gd_view_elapsed(&app->view, now));
        snprintf(footer, sizeof(footer), "%s%s %s", state, demo, age);
        canvas_draw_line(canvas, 0, 53, 127, 53);
        canvas_draw_str(canvas, 2, 63, footer);
    }
    furi_mutex_release(app->model_mutex);
}

static void input(InputEvent* event, void* context) {
    Glimdock* app = context;
    if(event->type == InputTypeShort) furi_message_queue_put(app->input_queue, event, 0);
}

static bool exit_signal(uint32_t signal, void* argument, void* context) {
    UNUSED(argument);
    if(signal != FuriSignalExit) return false;
    Glimdock* app = context;
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    app->exit_requested = true;
    furi_mutex_release(app->model_mutex);
    return true;
}

/* The service invokes this in the app task after complete-packet validation. */
static bool receive_frame(const uint8_t* frame, size_t size, void* context) {
    Glimdock* app = context;
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    uint32_t now = furi_get_tick();
    bool catalog = size >= 4 && frame[0] == 'G' && frame[1] == 'C';
    bool accepted = catalog ? gd_node_cache_accept_catalog(&app->cache, frame, size, now) != 0 :
                             gd_node_cache_accept_detail(&app->cache, frame, size, now);
    if(accepted) {
        app->connected = true;
        app->view.packet_ms = now;
        if(catalog && frame[3] == GD_CATALOG_COMMIT) app->selection_dirty = true;
    }
    furi_mutex_release(app->model_mutex);
    return accepted;
}

static void status_changed(BtStatus status, void* context) {
    Glimdock* app = context;
    /* Profile -> model is the only nesting order, matching receive_frame. */
    glimdock_ble_set_connected(app->profile, status == BtStatusConnected);
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    app->connected = status == BtStatusConnected;
    gd_node_cache_reset_transport(&app->cache);
    app->selection_dirty = true;
    furi_mutex_release(app->model_mutex);
    view_port_update(app->viewport);
}

/* Copy under the model lock, then touch the profile without holding that lock. */
static void queue_selection(Glimdock* app) {
    char node_id[GD_NODE_ID_BYTES];
    uint32_t sequence = 0;
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    bool dirty = app->selection_dirty;
    bool selected = dirty && gd_node_cache_selection(&app->cache, &sequence, node_id);
    app->selection_dirty = false;
    furi_mutex_release(app->model_mutex);
    if(!dirty) return;
    if(selected) {
        if(!glimdock_ble_request_node(app->profile, sequence, node_id)) {
            furi_mutex_acquire(app->model_mutex, FuriWaitForever);
            app->selection_dirty = true;
            furi_mutex_release(app->model_mutex);
        }
    } else glimdock_ble_clear_selection(app->profile);
}

int32_t glimdock_app(void* argument) {
    UNUSED(argument);
    Glimdock* app = calloc(1, sizeof(Glimdock));
    furi_check(app);
    gd_node_cache_init(&app->cache);
    app->model_mutex = furi_mutex_alloc(FuriMutexTypeNormal);
    app->input_queue = furi_message_queue_alloc(8, sizeof(InputEvent));
    furi_thread_set_signal_callback(furi_thread_get_current(), exit_signal, app);
    app->viewport = view_port_alloc();
    view_port_draw_callback_set(app->viewport, draw, app);
    view_port_input_callback_set(app->viewport, input, app);
    Gui* gui = furi_record_open(RECORD_GUI);
    gui_add_view_port(gui, app->viewport, GuiLayerFullscreen);
    app->bt = furi_record_open(RECORD_BT);

    Storage* storage = furi_record_open(RECORD_STORAGE);
    /* The resolver creates the app directory. mkdir("/data/") asks FAT to
     * create an alias root with a trailing slash and can reject a healthy SD. */
    FuriString* data_path = furi_string_alloc_set(STORAGE_APP_DATA_PATH_PREFIX);
    storage_common_resolve_path_and_ensure_app_directory(storage, data_path);
    FileInfo data_info;
    FS_Error data_error = storage_common_stat(storage, furi_string_get_cstr(data_path), &data_info);
    bool data_ready = data_error == FSE_OK && file_info_is_dir(&data_info);
    furi_string_free(data_path);
    furi_record_close(RECORD_STORAGE);
    bool changed_profile = false;
    if(data_ready) {
        bt_disconnect(app->bt);
        furi_delay_ms(200); /* Same controller/NVM barrier as native BT Remote. */
        bt_keys_storage_set_storage_path(app->bt, APP_DATA_PATH(".bt.keys"));
        changed_profile = true;
        app->profile = glimdock_ble_start(app->bt, receive_frame, app);
        if(app->profile) {
            bt_set_status_changed_callback(app->bt, status_changed, app);
            furi_hal_bt_start_advertising();
        }
    }
    furi_mutex_acquire(app->model_mutex, FuriWaitForever);
    if(!data_ready) {
        app->storage_error = true;
        if(data_error == FSE_OK) strlcpy(app->error, "App data path is not a folder.", sizeof(app->error));
        else snprintf(app->error, sizeof(app->error), "App data: %s", filesystem_api_error_get_desc(data_error));
    }
    else if(!app->profile) strlcpy(app->error, "Cannot start BLE profile.", sizeof(app->error));
    app->startup_complete = true;
    furi_mutex_release(app->model_mutex);

    uint32_t last_redraw = 0;
    while(true) {
        queue_selection(app);
        glimdock_ble_poll(app->profile);
        queue_selection(app);
        furi_mutex_acquire(app->model_mutex, FuriWaitForever);
        bool exit_requested = app->exit_requested;
        furi_mutex_release(app->model_mutex);
        if(exit_requested) break;
        InputEvent event;
        bool got_input = furi_message_queue_get(app->input_queue, &event, furi_ms_to_ticks(20)) == FuriStatusOk;
        if(got_input) {
            if(event.key == InputKeyBack) break;
            if(event.key == InputKeyUp || event.key == InputKeyDown) {
                char node_id[GD_NODE_ID_BYTES];
                uint32_t sequence;
                furi_mutex_acquire(app->model_mutex, FuriWaitForever);
                bool accepted = gd_node_cache_select_step(&app->cache,
                    event.key == InputKeyUp ? -1 : 1, furi_get_tick(), &sequence, node_id);
                if(accepted) app->selection_dirty = true;
                furi_mutex_release(app->model_mutex);
                if(accepted) {
                    view_port_update(app->viewport);
                    last_redraw = furi_get_tick();
                }
                continue;
            }
            furi_mutex_acquire(app->model_mutex, FuriWaitForever);
            if(event.key == InputKeyRight) gd_view_move(&app->view, 1);
            else if(event.key == InputKeyLeft) gd_view_move(&app->view, -1);
            else if(event.key == InputKeyOk) app->view.selected = 0;
            furi_mutex_release(app->model_mutex);
        }
        uint32_t now = furi_get_tick();
        if(got_input || (uint32_t)(now - last_redraw) >= 100) {
            view_port_update(app->viewport);
            last_redraw = now;
        }
    }

    furi_thread_set_signal_callback(furi_thread_get_current(), NULL, NULL);
    if(changed_profile) {
        glimdock_ble_prepare_stop(app->profile);
        bt_set_status_changed_callback(app->bt, NULL, NULL);
        /* Queue barriers and GapIdle precede handler removal/profile unload.
         * Do not hold model/profile locks across these synchronous BT calls. */
        bt_disconnect(app->bt);
        furi_delay_ms(200);
        furi_hal_bt_stop_advertising();
        bt_keys_storage_set_default_path(app->bt);
        bool restored = bt_profile_restore_default(app->bt);
        if(!restored) FURI_LOG_E("Glimdock", "Default Bluetooth profile unavailable");
        app->profile = NULL;
    }
    furi_record_close(RECORD_BT);
    view_port_enabled_set(app->viewport, false);
    gui_remove_view_port(gui, app->viewport);
    view_port_free(app->viewport);
    furi_record_close(RECORD_GUI);
    furi_message_queue_free(app->input_queue);
    furi_mutex_free(app->model_mutex);
    free(app);
    return 0;
}
