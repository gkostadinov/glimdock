#include "glimdock_ble.h"
#include "flipper_ble_protocol.h"
#include "flipper_cache_protocol.h"
#include "glimdock_hci.h"
#include <furi.h>
#include <furi_hal_bt.h>
#include <furi_ble/event_dispatcher.h>
#include <furi_ble/gatt.h>
#include <ble/core/ble_defs.h>
#include <stdlib.h>

typedef struct {
    GlimdockFrameCallback callback;
    void* context;
} GlimdockBleParameters;

struct GlimdockBle {
    FuriHalBleProfileBase base;
    FuriMutex* mutex;
    GapSvcEventHandler* event_handler;
    uint16_t service_handle;
    BleGattCharacteristicInstance write;
    BleGattCharacteristicInstance ack;
    BleGattCharacteristicInstance control;
    GdBleAssembler receiver;
    GlimdockFrameCallback callback;
    void* context;
    uint8_t ack_value[GD_BLE_ACK_BYTES];
    uint8_t control_value[GD_CACHE_CONTROL_BYTES];
    uint32_t control_sequence;
    size_t pending_size;
    bool control_pending;
    bool control_available;
    bool ack_pending;
    bool frame_pending;
    bool stopping;
    bool connected;
    bool stream_ready;
    uint32_t generation;
};

static const FuriHalBleProfileTemplate template;

static const BleGattCharacteristicParams write_parameters = {
    .name = "Glimdock status",
    .data_prop_type = FlipperGattCharacteristicDataFixed,
    .data.fixed.length = GD_BLE_CHUNK_MAX,
    .uuid.Char_UUID_128 = GD_BLE_WRITE_UUID_BYTES,
    .uuid_type = UUID_TYPE_128,
    .char_properties = CHAR_PROP_WRITE,
    .security_permissions = ATTR_PERMISSION_AUTHEN_WRITE,
    .gatt_evt_mask = GATT_NOTIFY_ATTRIBUTE_WRITE,
    .is_variable = CHAR_VALUE_LEN_VARIABLE,
};

static BleEventAckStatus service_event(void* event, void* context) {
    GlimdockBle* profile = context;
    /* Controller events always contain the three-byte HCI envelope; the SDK
     * does not export its private aggregate wrapper types. */
    const uint8_t* bytes = event;
    GdHciModified modified;
    if(!gd_hci_attribute_modified(bytes, (size_t)bytes[2] + 3, &modified)) return BleEventNotAck;
    if(modified.handle == profile->ack.handle + 2) return BleEventAckFlowEnable;
    if(modified.handle == profile->control.handle + 2) return BleEventAckFlowEnable;
    if(modified.handle != profile->write.handle + 1) return BleEventNotAck;

    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    if(!profile->stopping && !profile->frame_pending) {
        GdBleChunkResult result;
        if(modified.offset) {
            gd_ble_reset(&profile->receiver);
            result = GdBleChunkRejected;
        } else if(modified.size >= 8 && modified.size <= GD_BLE_CHUNK_MAX &&
                  modified.data[0] == 'G' && modified.data[1] == 'C') {
            /* Catalog records are individually acknowledged, never fragments. */
            gd_ble_reset(&profile->receiver);
            memcpy(profile->receiver.frame, modified.data, modified.size);
            profile->receiver.sequence = gd_ble_read_u32(modified.data + 4);
            profile->pending_size = modified.size;
            result = GdBleChunkComplete;
        } else {
            result = gd_ble_chunk_receive(&profile->receiver, modified.data, modified.size);
            if(result == GdBleChunkComplete) profile->pending_size = GD_CACHE_DETAIL_BYTES;
        }
        if(result == GdBleChunkComplete) {
            /* Validation, view publication and ACK happen in the app task;
             * the controller event thread only performs bounded reassembly. */
            profile->frame_pending = true;
        }
    }
    furi_mutex_release(profile->mutex);
    return BleEventAckFlowEnable;
}

static FuriHalBleProfileBase* profile_start(FuriHalBleProfileParams parameters) {
    const GlimdockBleParameters* options = parameters;
    GlimdockBle* profile = calloc(1, sizeof(GlimdockBle));
    if(!profile) return NULL;
    profile->base.config = &template;
    profile->mutex = furi_mutex_alloc(FuriMutexTypeNormal);
    profile->callback = options->callback;
    profile->context = options->context;
    Service_UUID_t service_uuid = {.Service_UUID_128 = GD_BLE_SERVICE_UUID_BYTES};
    if(!ble_gatt_service_add(UUID_TYPE_128, &service_uuid, PRIMARY_SERVICE, 9,
                             &profile->service_handle)) {
        furi_mutex_free(profile->mutex);
        free(profile);
        return NULL;
    }
    ble_gatt_characteristic_init(profile->service_handle, &write_parameters, &profile->write);
    BleGattCharacteristicParams ack_parameters = {
        .name = "Glimdock acknowledgement",
        .data_prop_type = FlipperGattCharacteristicDataFixed,
        .data.fixed = {.ptr = profile->ack_value, .length = GD_BLE_ACK_BYTES},
        .uuid.Char_UUID_128 = GD_BLE_ACK_UUID_BYTES,
        .uuid_type = UUID_TYPE_128,
        .char_properties = CHAR_PROP_READ | CHAR_PROP_NOTIFY,
        .security_permissions = ATTR_PERMISSION_AUTHEN_READ,
        .gatt_evt_mask = GATT_NOTIFY_ATTRIBUTE_WRITE,
        .is_variable = CHAR_VALUE_LEN_CONSTANT,
    };
    ble_gatt_characteristic_init(profile->service_handle, &ack_parameters, &profile->ack);
    BleGattCharacteristicParams control_parameters = {
        .name = "Glimdock node selection",
        .data_prop_type = FlipperGattCharacteristicDataFixed,
        .data.fixed = {.ptr = profile->control_value, .length = GD_CACHE_CONTROL_BYTES},
        .uuid.Char_UUID_128 = GD_BLE_CONTROL_UUID_BYTES,
        .uuid_type = UUID_TYPE_128,
        .char_properties = CHAR_PROP_READ | CHAR_PROP_NOTIFY,
        .security_permissions = ATTR_PERMISSION_AUTHEN_READ,
        .gatt_evt_mask = GATT_NOTIFY_ATTRIBUTE_WRITE,
        .is_variable = CHAR_VALUE_LEN_CONSTANT,
    };
    ble_gatt_characteristic_init(profile->service_handle, &control_parameters, &profile->control);
    profile->event_handler = ble_event_dispatcher_register_svc_handler(service_event, profile);
    return &profile->base;
}

static void profile_stop(FuriHalBleProfileBase* base) {
    GlimdockBle* profile = (GlimdockBle*)base;
    furi_check(base && base->config == &template);
    /* The app has disconnected, waited for controller/NVM completion and stopped
     * advertising before synchronous bt_profile_restore_default reaches here. */
    glimdock_ble_prepare_stop(profile);
    ble_event_dispatcher_unregister_svc_handler(profile->event_handler);
    ble_gatt_characteristic_delete(profile->service_handle, &profile->control);
    ble_gatt_characteristic_delete(profile->service_handle, &profile->ack);
    ble_gatt_characteristic_delete(profile->service_handle, &profile->write);
    ble_gatt_service_delete(profile->service_handle);
    furi_mutex_free(profile->mutex);
    free(profile);
}

static void profile_gap_config(GapConfig* config, FuriHalBleProfileParams parameters) {
    UNUSED(parameters);
    memset(config, 0, sizeof(*config));
    config->adv_service.UUID_Type = UUID_TYPE_16;
    config->adv_service.Service_UUID_16 = GD_BLE_ADVERTISED_SERVICE;
    config->appearance_char = 0x8600;
    config->bonding_mode = true;
    config->pairing_method = GapPairingPinCodeShow;
    config->conn_param.conn_int_min = 0x06;
    config->conn_param.conn_int_max = 0x24;
    memcpy(config->mac_address, furi_hal_version_get_ble_mac(), sizeof(config->mac_address));
    /* Distinct stable identity and separate key file preserve stock/mobile bonds. */
    config->mac_address[2] ^= 0x40;
    config->adv_name[0] = 0x09; /* AD_TYPE_COMPLETE_LOCAL_NAME, per GAP API. */
    strlcpy(config->adv_name + 1, GD_BLE_NAME, sizeof(config->adv_name) - 1);
}

static const FuriHalBleProfileTemplate template = {
    .start = profile_start,
    .stop = profile_stop,
    .get_gap_config = profile_gap_config,
};

GlimdockBle* glimdock_ble_start(Bt* bt, GlimdockFrameCallback callback, void* context) {
    GlimdockBleParameters parameters = {.callback = callback, .context = context};
    return (GlimdockBle*)bt_profile_start(bt, &template, &parameters);
}

void glimdock_ble_reset(GlimdockBle* profile) {
    glimdock_ble_set_connected(profile, false);
}

void glimdock_ble_set_connected(GlimdockBle* profile, bool connected) {
    if(!profile) return;
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    gd_ble_reset(&profile->receiver);
    profile->ack_pending = false;
    profile->frame_pending = false;
    profile->connected = connected;
    profile->stream_ready = false;
    profile->control_pending = profile->control_available;
    profile->generation++;
    furi_mutex_release(profile->mutex);
}

void glimdock_ble_prepare_stop(GlimdockBle* profile) {
    if(!profile) return;
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    profile->stopping = true;
    profile->connected = false;
    profile->stream_ready = false;
    profile->control_pending = false;
    profile->ack_pending = false;
    profile->frame_pending = false;
    profile->generation++;
    gd_ble_reset(&profile->receiver);
    furi_mutex_release(profile->mutex);
}

bool glimdock_ble_request_node(GlimdockBle* profile, uint32_t sequence, const char* node_id) {
    if(!profile) return false;
    uint8_t selection[GD_CACHE_CONTROL_BYTES];
    if(!gd_cache_control_encode(sequence, node_id, selection, sizeof(selection))) return false;
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    bool queued = !profile->stopping;
    if(queued) {
        memcpy(profile->control_value, selection, sizeof(selection));
        profile->control_sequence = sequence;
        profile->control_available = profile->control_pending = true;
    }
    furi_mutex_release(profile->mutex);
    return queued;
}

void glimdock_ble_clear_selection(GlimdockBle* profile) {
    if(!profile) return;
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    profile->control_available = profile->control_pending = false;
    memset(profile->control_value, 0, sizeof(profile->control_value));
    furi_mutex_release(profile->mutex);
}

void glimdock_ble_poll(GlimdockBle* profile) {
    if(!profile) return;
    uint8_t ack[GD_BLE_ACK_BYTES];
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    bool received_packet = !profile->stopping && profile->frame_pending;
    if(!profile->stopping && profile->frame_pending) {
        uint32_t packet_sequence;
        if(gd_cache_packet_sequence(profile->receiver.frame, profile->pending_size, &packet_sequence) &&
           packet_sequence == profile->receiver.sequence &&
           profile->callback(profile->receiver.frame, profile->pending_size, profile->context)) {
            gd_ble_ack_encode(packet_sequence, profile->ack_value);
            profile->ack_pending = true;
            profile->connected = true;
            GdCatalogPacket catalog;
            if(gd_catalog_decode(profile->receiver.frame, profile->pending_size, &catalog) &&
               catalog.type == GD_CATALOG_COMMIT) profile->stream_ready = true;
        }
        profile->frame_pending = false;
        gd_ble_reset(&profile->receiver);
    }
    bool pending = !profile->stopping && profile->ack_pending;
    uint32_t generation = profile->generation;
    if(pending) {
        memcpy(ack, profile->ack_value, sizeof(ack));
    }
    furi_mutex_release(profile->mutex);
    if(pending) {
        /* Unlike service_add, this pinned SDK function returns true on error. */
        bool update_failed = ble_gatt_characteristic_update(profile->service_handle, &profile->ack, ack);
        furi_mutex_acquire(profile->mutex, FuriWaitForever);
        if(!profile->stopping && generation == profile->generation &&
           !memcmp(ack, profile->ack_value, sizeof(ack))) profile->ack_pending = update_failed;
        furi_mutex_release(profile->mutex);
    }
    /* Let the app reconcile a committed/reordered catalog before sending a
     * retained selection, which may have disappeared from the new catalog. */
    if(received_packet) return;
    uint8_t control_value[GD_CACHE_CONTROL_BYTES];
    furi_mutex_acquire(profile->mutex, FuriWaitForever);
    bool control_pending = !profile->stopping && profile->connected && profile->stream_ready &&
                           profile->control_available && profile->control_pending;
    uint32_t control_sequence = profile->control_sequence;
    generation = profile->generation;
    if(control_pending) {
        memcpy(control_value, profile->control_value, sizeof(control_value));
    }
    furi_mutex_release(profile->mutex);
    if(control_pending) {
        bool update_failed = ble_gatt_characteristic_update(profile->service_handle, &profile->control, control_value);
        furi_mutex_acquire(profile->mutex, FuriWaitForever);
        if(!update_failed && !profile->stopping && generation == profile->generation &&
           control_sequence == profile->control_sequence &&
           !memcmp(control_value, profile->control_value, sizeof(control_value)))
            profile->control_pending = false;
        furi_mutex_release(profile->mutex);
    }
}
