/* Production main.cpp and LVGL run unchanged; only board/network transport differ. */
#include "model.h"
#include "board.h"
#include <lvgl.h>
#include <ArduinoJson.h>
#include <emscripten.h>
#include <array>
#include <string>
#include <cstdlib>

void homelabPreviewInit(Snapshot *, NetworkState *);
void homelabPreviewShow(uint8_t);
void homelabPreviewRefresh();
void homelabPreviewPoll();
void homelabPreviewTick();
void homelabPreviewScroll(int);
uint8_t homelabPreviewPage();
bool homelabPreviewBusy();

uint32_t webClock=1000;
uint32_t millis(){return webClock;}
Snapshot *publishedSnapshot=nullptr;
SemaphoreHandle_t dataMutex=nullptr;
NetworkState networkState;
volatile bool requestSetup=false;
void boardHoldPower(){}void boardInit(){}void boardLvglInit(){}void startNetwork(){}
uint8_t webBrightness=80;
void boardBacklight(uint8_t value){webBrightness=value;}
bool readSnapshot(Snapshot&out,NetworkState&state){if(!publishedSnapshot)return false;out=*publishedSnapshot;state=networkState;return true;}
extern "C" void*homelab_lvgl_pool(size_t bytes){static void*pool=std::calloc(1,bytes);return pool;}

#include "firmware_parser.h"

EM_JS(void, webRequest, (const char *kind, const char *payload), {
  const request={kind:UTF8ToString(kind),payload:UTF8ToString(payload)};
  queueMicrotask(()=>Module.onRequest?.(request));
});

namespace {
constexpr int width=HOMELAB_VIEWPORT_WIDTH,height=HOMELAB_VIEWPORT_HEIGHT;
std::array<uint16_t,width*height> framebuffer{};
alignas(4) uint8_t drawBuffer[width*24*2];
Snapshot transportSnapshot,uiSnapshot;
NodeConfiguration configuration;
ConnectionSettings connection;
lv_display_t*display=nullptr;
lv_indev_t*pointer=nullptr;
lv_indev_data_t sample{};
uint32_t frameRevision=0;
std::string labelJson;

void flush(lv_display_t*d,const lv_area_t*a,uint8_t*p){
  const auto*source=reinterpret_cast<const uint16_t*>(p);int stride=a->x2-a->x1+1;
  for(int y=a->y1;y<=a->y2;y++)for(int x=a->x1;x<=a->x2;x++)framebuffer[y*width+x]=source[(y-a->y1)*stride+x-a->x1];
  frameRevision++;lv_display_flush_ready(d);
}
void input(lv_indev_t*,lv_indev_data_t*data){*data=sample;noteTouch(sample.state==LV_INDEV_STATE_PRESSED);if(consumeWakeTouch)data->state=LV_INDEV_STATE_RELEASED;}
void configMessage(const char*message){configuration.busy=false;configuration.applying=false;configuration.saved=false;strlcpy(configuration.message,message,sizeof(configuration.message));configuration.revision++;networkState.revision++;}
void labels(lv_obj_t*object,JsonArray out){
  if(lv_obj_has_flag(object,LV_OBJ_FLAG_HIDDEN))return;
  if(lv_obj_check_type(object,&lv_label_class)){lv_area_t a;lv_obj_get_coords(object,&a);auto item=out.add<JsonObject>();item["text"]=lv_label_get_text(object);item["x"]=a.x1;item["y"]=a.y1;item["width"]=a.x2-a.x1+1;item["height"]=a.y2-a.y1+1;}
  for(uint32_t i=0;i<lv_obj_get_child_count(object);i++)labels(lv_obj_get_child(object,i),out);
}
}

bool requestNodeSelection(const char*id){
  int index=publishedSnapshot?nodeIndexForId(*publishedSnapshot,id):-1;if(index<0)return false;
  if(publishedSnapshot->valid&&!strcmp(id,publishedSnapshot->node.id))return true;
  clearForNode(*publishedSnapshot,publishedSnapshot->nodes[index]);networkState.loading=true;strlcpy(networkState.message,"Loading selected node",sizeof(networkState.message));networkState.revision++;webRequest("select",id);return true;
}
bool readConnectionSettings(ConnectionSettings&out){out=connection;return true;}
bool requestConnectionUpdate(const ConnectionUpdate&){
  connection.busy=false;connection.saved=false;strlcpy(connection.message,"Browser transport: use the web interface to pair with your collector. Device Wi-Fi is configured on the display.",sizeof(connection.message));connection.revision++;return true;
}
bool readNodeConfiguration(NodeConfiguration&out){out=configuration;return true;}
bool requestNodeConfiguration(){
  if(configuration.busy)return false;configuration.busy=true;configuration.saved=false;configuration.applying=false;strlcpy(configuration.message,"Loading collector settings...",sizeof(configuration.message));configuration.revision++;webRequest("config","");return true;
}
bool requestNodeUpsert(const NodeConfigDraft&draft,const char*version){
  if(configuration.busy||!version||strlen(version)!=64||(draft.node.id[0]&&!validNodeId(draft.node.id)))return false;
  JsonDocument request;request["action"]="upsert";request["version"]=version;auto node=request["node"].to<JsonObject>();
  node["id"]=draft.node.id;node["type"]=agentNodeType(draft.node.type)?"server":draft.node.type;node["name"]=draft.node.name;node["enabled"]=draft.node.enabled;if(agentNodeType(draft.node.type))node["origin"]="agent";
  if(localNodeType(draft.node.type)){if(!deviceNodeType(draft.node.type))node["platform"]="linux";else if(draft.node.platform[0])node["platform"]=draft.node.platform;}else if(deviceNodeType(draft.node.type))node["platform"]=draft.node.platform;
  if(agentNodeType(draft.node.type)){node["poll_interval_s"]=draft.node.poll;node["ttl_s"]=draft.node.ttl;}else if(!localNodeType(draft.node.type)){node["url"]=draft.node.url;node["poll_interval_s"]=draft.node.poll;node["timeout_s"]=draft.node.timeout;node["ttl_s"]=draft.node.ttl;if(draft.secret[0])node["secret"]=draft.secret;if(draft.clearSecret)node["clear_secret"]=true;}
  if(measureJson(request)>4096)return false;std::string payload;serializeJson(request,payload);
  configuration.busy=true;configuration.applying=false;configuration.saved=false;configuration.revision++;strlcpy(configuration.message,"Saving node settings...",sizeof(configuration.message));webRequest("update",payload.c_str());return true;
}
bool requestNodeDelete(const char*id,const char*version){
  if(configuration.busy||!validNodeId(id)||!version||strlen(version)!=64)return false;JsonDocument request;request["action"]="delete";request["id"]=id;request["version"]=version;std::string payload;serializeJson(request,payload);
  configuration.busy=true;configuration.applying=false;configuration.saved=false;configuration.revision++;strlcpy(configuration.message,"Removing node...",sizeof(configuration.message));webRequest("update",payload.c_str());return true;
}

extern "C" {
EMSCRIPTEN_KEEPALIVE int glimdock_width(){return width;}
EMSCRIPTEN_KEEPALIVE int glimdock_height(){return height;}
EMSCRIPTEN_KEEPALIVE uintptr_t glimdock_pixels(){return reinterpret_cast<uintptr_t>(framebuffer.data());}
EMSCRIPTEN_KEEPALIVE uint32_t glimdock_frame(){return frameRevision;}
EMSCRIPTEN_KEEPALIVE int glimdock_page(){return homelabPreviewPage();}
EMSCRIPTEN_KEEPALIVE int glimdock_brightness(){return webBrightness;}
EMSCRIPTEN_KEEPALIVE int glimdock_select(const char*id){if(!requestNodeSelection(id))return 0;homelabPreviewPoll();homelabPreviewShow(0);lv_refr_now(display);return 1;}
EMSCRIPTEN_KEEPALIVE void glimdock_default(){NodeDescriptor unknown;clearForNode(transportSnapshot,unknown);networkState.loading=true;strlcpy(networkState.message,"Node removed; loading default",sizeof(networkState.message));networkState.revision++;}
EMSCRIPTEN_KEEPALIVE void glimdock_tick(uint32_t now){webClock=now;homelabPreviewTick();lv_timer_handler();}
EMSCRIPTEN_KEEPALIVE void glimdock_pointer(int x,int y,int down){sample.point.x=constrain(x,0,width-1);sample.point.y=constrain(y,0,height-1);sample.state=down?LV_INDEV_STATE_PRESSED:LV_INDEV_STATE_RELEASED;lv_indev_read(pointer);lv_timer_handler();}
EMSCRIPTEN_KEEPALIVE int glimdock_snapshot(const char*body){
  if(!body||strlen(body)>MAX_JSON)return 0;JsonDocument doc;auto error=deserializeJson(doc,body,DeserializationOption::NestingLimit(12));if(error||doc.overflowed())return 0;
  static Snapshot next;if(!firmware_json::parse(doc,next))return 0;if(transportSnapshot.valid&&!strcmp(transportSnapshot.node.id,next.node.id)&&transportSnapshot.sequence==next.sequence)next.received=transportSnapshot.received;transportSnapshot=next;networkState.loading=false;networkState.wifi=true;strlcpy(networkState.message,"Collector connected",sizeof(networkState.message));networkState.revision++;
  if(configuration.applying)settleNodeConfiguration(configuration);return 1;
}
EMSCRIPTEN_KEEPALIVE void glimdock_error(const char*message){networkState.loading=false;strlcpy(networkState.message,message?message:"Collector unreachable",sizeof(networkState.message));networkState.revision++;}
EMSCRIPTEN_KEEPALIVE void glimdock_connection(const char*endpoint,int displayPaired,int setupPaired){connection.available=true;strlcpy(connection.ssid,"Browser connection",sizeof(connection.ssid));strlcpy(connection.endpoint,endpoint?endpoint:"",sizeof(connection.endpoint));connection.displayTokenSaved=displayPaired;connection.setupTokenSaved=setupPaired;connection.revision++;}
EMSCRIPTEN_KEEPALIVE int glimdock_config(const char*body,int status){
  if(!body||strlen(body)>MAX_CONFIG_JSON){configMessage("Collector settings response is too large.");return 0;}JsonDocument doc;auto error=deserializeJson(doc,body,DeserializationOption::NestingLimit(8));if(error){configMessage("Collector returned invalid settings data.");return 0;}
  if(status!=200&&status!=202){configMessage(status==401||status==403?"Setup token rejected. Pair the separate configuration token.":status==409?"Settings changed elsewhere. Refresh before saving again.":doc["error"]|"Node settings were rejected.");return 0;}
  NodeConfiguration next;auto publicConfig=doc["config"].is<JsonObjectConst>()?doc["config"].as<JsonObjectConst>():doc.as<JsonObjectConst>();
  if(!firmware_json::parseConfiguration(publicConfig,next)){configMessage("Unsupported collector settings schema.");return 0;}next.revision=configuration.revision+1;next.applying=status==202;next.saved=status==202;if(next.applying)strlcpy(next.message,"Saved. Collector is applying node settings...",sizeof(next.message));configuration=next;networkState.revision++;
  if(next.applying){
    NodeDescriptor selected=configuredRegistry(transportSnapshot,next,transportSnapshot.node.id);
    clearForNode(transportSnapshot,selected);networkState.loading=transportSnapshot.nNodes>0;strlcpy(networkState.message,transportSnapshot.nNodes?"Applying node settings...":"No nodes configured; open Settings",sizeof(networkState.message));
  }
  return 1;
}
EMSCRIPTEN_KEEPALIVE void glimdock_show(int page){homelabPreviewShow(uint8_t(page));lv_refr_now(display);}
EMSCRIPTEN_KEEPALIVE void glimdock_scroll(int y){homelabPreviewScroll(y);lv_refr_now(display);}
EMSCRIPTEN_KEEPALIVE const char*glimdock_labels(){lv_obj_update_layout(lv_screen_active());JsonDocument doc;labels(lv_screen_active(),doc.to<JsonArray>());labelJson.clear();serializeJson(doc,labelJson);return labelJson.c_str();}
}

int main(){
  networkState.configured=true;networkState.wifi=true;networkState.loading=true;strlcpy(networkState.message,"Connecting to collector...",sizeof(networkState.message));publishedSnapshot=&transportSnapshot;
  lv_init();lv_tick_set_cb(millis);display=lv_display_create(width,height);lv_display_set_color_format(display,LV_COLOR_FORMAT_RGB565);lv_display_set_flush_cb(display,flush);lv_display_set_buffers(display,drawBuffer,nullptr,sizeof(drawBuffer),LV_DISPLAY_RENDER_MODE_PARTIAL);
  homelabPreviewInit(&uiSnapshot,&networkState);pointer=lv_indev_create();lv_indev_set_type(pointer,LV_INDEV_TYPE_POINTER);lv_indev_set_read_cb(pointer,input);lv_indev_set_scroll_limit(pointer,8);lv_refr_now(display);return 0;
}
