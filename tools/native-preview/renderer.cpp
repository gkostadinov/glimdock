/* Render the production firmware UI through LVGL's real software rasterizer. */
#include "model.h"
#include "board.h"
#include <lvgl.h>
#include <fixture.h>
#include <fstream>
#include <filesystem>
#include <iostream>
#include <array>
#include <ctime>
#include <set>
#include <vector>
#include <sstream>

void homelabPreviewInit(Snapshot *, NetworkState *);
void homelabPreviewShow(uint8_t);
void homelabPreviewScroll(int);
void homelabPreviewTheme(bool);
void homelabPreviewSensorFilter(uint8_t);
void homelabPreviewGuest(int);
void homelabPreviewGpu(uint8_t);
void homelabPreviewPoll();
uint32_t nativeClock = 1000;
uint32_t millis() { return nativeClock; }
Snapshot *publishedSnapshot = nullptr;
SemaphoreHandle_t dataMutex = nullptr;
NetworkState networkState;
volatile bool requestSetup = false;
void boardInit() {}
void boardBacklight(uint8_t) {}
void boardLvglInit() {}
void startNetwork() {}
bool readSnapshot(Snapshot&out,NetworkState&state){if(!publishedSnapshot)return false;out=*publishedSnapshot;state=networkState;return true;}
void makeDemo(Snapshot &out) { loadFixture(out, networkState); }
bool requestNodeSelection(const char*id){if(!publishedSnapshot||!validNodeId(id))return false;for(int i=0;i<publishedSnapshot->nNodes;i++)if(!strcmp(id,publishedSnapshot->nodes[i].id)){clearForNode(*publishedSnapshot,publishedSnapshot->nodes[i]);networkState.loading=true;strlcpy(networkState.message,"Loading selected node",sizeof(networkState.message));networkState.revision++;return true;}return false;}
#include "native_management.h"
extern "C" void *homelab_lvgl_pool(size_t size) {
  static void *pool = std::calloc(1, size);
  return pool;
}

namespace {
constexpr int width = HOMELAB_VIEWPORT_WIDTH, height = HOMELAB_VIEWPORT_HEIGHT;
std::array<uint16_t, width * height> framebuffer{};
alignas(4) uint8_t drawBuffer[width * 24 * 2];
void flush(lv_display_t *display, const lv_area_t *area, uint8_t *pixels) {
  const uint16_t *source = reinterpret_cast<uint16_t *>(pixels);
  int stride = area->x2 - area->x1 + 1;
  for (int y = area->y1; y <= area->y2; ++y)
    for (int x = area->x1; x <= area->x2; ++x)
      framebuffer[y * width + x] = source[(y - area->y1) * stride + x - area->x1];
  lv_display_flush_ready(display);
}
void save(const std::filesystem::path &path, lv_display_t *display) {
  nativeClock += 25;
  lv_timer_handler();
  lv_refr_now(display);
  std::ofstream out(path, std::ios::binary);
  out << "P6\n" << width << " " << height << "\n255\n";
  for (uint16_t value : framebuffer) {
    const uint8_t pixel[3] = {uint8_t(((value >> 11) & 31) * 255 / 31), uint8_t(((value >> 5) & 63) * 255 / 63), uint8_t((value & 31) * 255 / 31)};
    out.write(reinterpret_cast<const char *>(pixel), 3);
  }
  lv_mem_monitor_t memory;
  lv_mem_monitor(&memory);
  std::cout << path.filename().string() << ": LVGL used " << unsigned(memory.used_pct) << "% / " << memory.total_size << " bytes; largest free " << memory.free_biggest_size << " bytes\n";
}
}
int main(int argc, char **argv) {
  if (argc < 2 || argc > 3) { std::cerr << "Usage: native-preview OUTPUT_DIRECTORY [PAGE_NUMBERS]\n"; return 2; }
  std::set<int> selected;
  if (argc == 3) { std::stringstream input(argv[2]); std::string part; while(std::getline(input,part,',')) selected.insert(std::stoi(part)); }
  std::filesystem::path output = argv[1];
  std::filesystem::create_directories(output);
  static Snapshot data;
  loadFixture(data, networkState);
  std::cout << "fixture_sha256=" << fixtureFingerprint() << "\n";
  std::cout << "viewport=" << width << "x" << height << " rotation=" << HOMELAB_ROTATION << "\n";
  std::cout << "Fixture " << data.host << ": " << unsigned(data.nGuests) << " guests, " << unsigned(data.nPools) << " storage, " << unsigned(data.nDisks) << " disks, " << unsigned(data.nSensors) << " sensors, " << unsigned(data.nFaults) << " faults\n";
  double rebase = double(std::time(nullptr)) - data.generated;
  data.generated = std::time(nullptr);
  if(std::isfinite(data.node.summary.generated))data.node.summary.generated+=rebase;
  for(int i=0;i<data.nNodes;i++)if(std::isfinite(data.nodes[i].summary.generated))data.nodes[i].summary.generated+=rebase;
  for(int i=0;i<data.nGuests;i++)if(std::isfinite(data.guests[i].memUpdated))data.guests[i].memUpdated+=rebase;
  for(int i=0;i<data.nGpus;i++)if(std::isfinite(data.gpus[i].updated))data.gpus[i].updated+=rebase;
  if(std::isfinite(data.printer.updated))data.printer.updated+=rebase;if(std::isfinite(data.printer.eta))data.printer.eta+=rebase;
  data.received = nativeClock;
  lv_init();
  lv_tick_set_cb(millis);
  lv_display_t *display = lv_display_create(width, height);
  lv_display_set_color_format(display, LV_COLOR_FORMAT_RGB565);
  lv_display_set_flush_cb(display, flush);
  lv_display_set_buffers(display, drawBuffer, nullptr, sizeof(drawBuffer), LV_DISPLAY_RENDER_MODE_PARTIAL);
  publishedSnapshot=&data;homelabPreviewInit(&data, &networkState);
  const bool printer=!strcmp(data.node.type,"klipper");
  std::vector<std::pair<uint8_t,const char*>> pages = {{19,"all-nodes"},{0,"overview"},{1,"guests"},{4,"guest-detail"},{2,"storage"},{9,"disk-detail"},{3,"sensors"},{5,"power"},{10,"memory"},{11,"gpus"},{12,"gpu-detail"},{6,"alerts"},{7,"settings"},{13,"nodes"}};
  if(deviceNodeType(data.node.type))pages={{19,"all-nodes"},{0,"overview"},{2,"storage"},{9,"disk-detail"},{3,"sensors"},{5,"power"},{10,"memory"},{11,"gpus"},{12,"gpu-detail"},{6,"health"},{7,"settings"},{13,"nodes"}};
  if(printer)pages={{19,"all-nodes"},{0,"overview"},{1,"job"},{2,"temps"},{3,"health"},{13,"nodes"},{7,"settings"}};
  for(auto item:std::vector<std::pair<uint8_t,const char*>>{{14,"wifi"},{15,"node-settings"},{16,"node-edit"},{17,"node-delete"},{18,"keyboard"}})if(selected.empty()||selected.count(item.first))pages.push_back(item);
  if(selected.count(8))pages.push_back({8,"setup"});
  for (auto item : pages) {
    if (!selected.empty() && !selected.count(item.first)) continue;
    homelabPreviewShow(item.first); homelabPreviewScroll(0);
    save(output / (std::string(item.second) + ".ppm"), display);
    if (item.first == 0 && !printer) {
      uint32_t clockBefore = nativeClock, receivedBefore = data.received;
      nativeClock = data.received = 6500; homelabPreviewShow(0); save(output / "overview-inventory.ppm",display);
      nativeClock = data.received = 12500; homelabPreviewShow(0); save(output / "overview-thermals.ppm",display);
      if(data.nGpus){nativeClock=data.received=18500;homelabPreviewShow(0);save(output/"overview-gpus.ppm",display);}
      nativeClock = clockBefore; data.received = receivedBefore;
    }
    if(item.first==3&&!printer){const std::pair<uint8_t,const char*>filters[]={{0,"all"},{2,"fans"},{3,"volts"},{4,"power"},{5,"amps"}};for(auto filter:filters){homelabPreviewSensorFilter(filter.first);homelabPreviewScroll(0);save(output/("sensors-"+std::string(filter.second)+".ppm"),display);if(filter.first==4){homelabPreviewScroll(100);save(output/"sensors-power-scrolled.ppm",display);}}homelabPreviewSensorFilter(1);homelabPreviewScroll(0);}
    if(item.first==4){for(int i=1;i<std::min(3,int(data.nGuests));i++){homelabPreviewGuest(data.guests[i].id);homelabPreviewScroll(0);save(output/("guest-"+std::to_string(data.guests[i].id)+".ppm"),display);homelabPreviewScroll(145);save(output/("guest-"+std::to_string(data.guests[i].id)+"-memory.ppm"),display);}homelabPreviewShow(4);homelabPreviewScroll(0);}
    if(item.first==12){for(int i=1;i<data.nGpus;i++){homelabPreviewGpu(i);homelabPreviewScroll(0);save(output/("gpu-detail-"+std::to_string(i+1)+".ppm"),display);homelabPreviewScroll(140);save(output/("gpu-detail-"+std::to_string(i+1)+"-scrolled.ppm"),display);}homelabPreviewShow(12);homelabPreviewScroll(0);}
    if (item.first != 0 && item.first != 18 && item.first != 19) {
      homelabPreviewScroll(printer?(item.first==1?155:item.first==2?100:item.first==3?190:120):item.first == 2 ? 180 : item.first == 5 ? 140 : item.first == 7 ? 150 : item.first == 6 ? 300 : item.first == 4 ? 145 : item.first == 9 ? 170 : item.first == 10 ? 180 : item.first == 12 ? 140 : 420);
      save(output / (std::string(item.second) + "-scrolled.ppm"), display);
      homelabPreviewScroll(100000);
      save(output / (std::string(item.second) + "-end.ppm"), display);
    }
  }
  homelabPreviewTheme(false);
  for(auto item:pages){if(!selected.empty()&&!selected.count(item.first))continue;homelabPreviewShow(item.first);homelabPreviewScroll(0);save(output/("dark-"+std::string(item.second)+".ppm"),display);if(item.first==3&&!printer){for(auto filter:std::array<std::pair<uint8_t,const char*>,2>{{{4,"power"},{5,"amps"}}}){homelabPreviewSensorFilter(filter.first);homelabPreviewScroll(0);save(output/("dark-sensors-"+std::string(filter.second)+".ppm"),display);if(filter.first==4){homelabPreviewScroll(100);save(output/"dark-sensors-power-scrolled.ppm",display);}}homelabPreviewSensorFilter(1);}}
  homelabPreviewTheme(true);
  if (selected.empty()) {
  data.demo = false;
  data.generated=std::time(nullptr);data.received=nativeClock;homelabPreviewShow(0);save(output/"live.ppm",display);
  homelabPreviewTheme(false);save(output/"dark-live.ppm",display);homelabPreviewTheme(true);
  data.generated -= 180; homelabPreviewShow(0); save(output / "stale.ppm",display);
  data.valid = false; homelabPreviewShow(0); save(output / "offline.ppm",display);
  strlcpy(networkState.message,"Collector connection timed out. Check the server address, display token and Wi-Fi and try again",sizeof(networkState.message));
  homelabPreviewPoll();homelabPreviewShow(0);homelabPreviewScroll(0);save(output/"offline-long-message.ppm",display);
  homelabPreviewScroll(100000);save(output/"offline-long-message-end.ppm",display);
  homelabPreviewTheme(false);homelabPreviewScroll(0);save(output/"dark-offline-long-message.ppm",display);homelabPreviewTheme(true);
  networkState.setup = true; strlcpy(networkState.apPassword,"PREVIEW12345",sizeof(networkState.apPassword));
  strlcpy(networkState.message,"Collector unreachable",sizeof(networkState.message));
  lv_obj_clean(lv_screen_active());
  homelabPreviewInit(&data,&networkState); homelabPreviewShow(8); save(output / "setup.ppm",display);
  }
  std::cout << "sizeof(Snapshot)=" << sizeof(Snapshot) << "\n";
}
