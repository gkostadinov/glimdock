#include <Arduino.h>
#include <Preferences.h>
#include <esp_heap_caps.h>
#include "runtime_health.h"
#include <lvgl.h>
#include <time.h>
#include <stdlib.h>
#include "board.h"
#include "model.h"
#include "config.h"
#include "brand.h"
#include "brand_assets.h"
#include "display_clock.h"
#include "audio.h"
#include "notification_policy.h"
#include "operations_model.h"
#include "private_chat_network.h"
#include "flipper_ble.h"
#include "flipper_status.h"
bool consumeWakeTouch=false;
namespace {
// The panel geometry is shared with the display/touch driver and native preview.
constexpr bool PORTRAIT=HOMELAB_IS_PORTRAIT;
constexpr int VIEW_W=HOMELAB_VIEWPORT_WIDTH, VIEW_H=HOMELAB_VIEWPORT_HEIGHT;
constexpr int HEADER_H=PORTRAIT?56:34, TAB_H=40, CONTENT_H=VIEW_H-HEADER_H-TAB_H;
constexpr int PAGE_W=VIEW_W-16, LIST_W=VIEW_W-24, TAB_W=VIEW_W/4;
constexpr int CELL_COLUMNS=PORTRAIT?1:2;
constexpr int cellRows(int count){return (count+CELL_COLUMNS-1)/CELL_COLUMNS;}
constexpr int cellBottom(int count,int top=0){return top+cellRows(count)*45;}
struct Palette {uint32_t bg,card,ink,muted,cyan,lavender,mint,amber,red,edge;};
constexpr Palette LIGHT={0xf2f5f8,0xffffff,0x101e30,0x526275,0x007b96,0x6047ba,0x00734c,0x8a5300,0xb32632,0xdce5ed};
// Lift the slate surfaces and small-label contrast for the 320x240 LCD.
constexpr Palette DARK={0x243246,0x3a4d63,0xf5f8fc,0xc2d2e3,0x54d9f2,0xc9b9ff,0x64e5b0,0xffbe68,0xff7184,0x71869e};
constexpr uint32_t DARK_SELECTED=0x405e72;
uint32_t BG=LIGHT.bg,CARD=LIGHT.card,INK=LIGHT.ink,MUTED=LIGHT.muted,CYAN=LIGHT.cyan,LAVENDER=LIGHT.lavender,MINT=LIGHT.mint,AMBER=LIGHT.amber,RED=LIGHT.red,EDGE=LIGHT.edge;
Snapshot *snapshot=nullptr;NetworkState net;
ops::Snapshot*operations=nullptr;ops::Network operationsNet;
bool agentsWorkspace=false;uint8_t savedNodesPage=19,savedAgentsPage=22;
lv_obj_t*workspaceHighlight=nullptr;int workspaceStep=60;
constexpr uint8_t OPS_OVERVIEW=22,OPS_AGENTS=23,OPS_TASKS=24,OPS_EVENTS=25,OPS_AGENT=26,OPS_TASK=27,OPS_EVENT=28,OPS_OPERATIONS=29,OPS_SOURCES=30,OPS_SYSTEMS=31;
constexpr uint8_t PRIVATE_SESSIONS=32,PRIVATE_MESSAGES=33;
constexpr uint8_t FLIPPER_PAGE=34;
flipper::State flipperUi{};
uint32_t lastFlipperPublish=0,flipperSequence=0;
char flipperRequestedNode[64]{};uint32_t flipperRequestSequence=0;
String flipperPinDraft;
bool privatePage(uint8_t p){return p==PRIVATE_SESSIONS||p==PRIVATE_MESSAGES;}
bool privateNewRequest=false,privateScrollToBottom=false;privatechat::Model*privateChat=nullptr;privatechat::State privateState;
void privateOpen(lv_event_t*);void privateOpenTask(lv_event_t*);void privateRender();void privateBack();void privateResetUi();void privateWipeLabels(lv_obj_t*);void privateHeader();
bool operationsPage(uint8_t p){return p>=OPS_OVERVIEW&&p<=OPS_SYSTEMS;}
void toggleWorkspace(lv_event_t*);void workspaceSwitchControl();void operationsRender();void operationsHeader();
int operationsTab();void operationsBack();bool operationsAll();void operationsHome();void operationsClockGo();String operationsClockSummary();void operationsClockExtras(int);
lv_obj_t *content=nullptr,*scrollArea=nullptr,*headerTitle=nullptr,*headerMark=nullptr,*headerHost=nullptr,*nodeButton=nullptr,*headerLine=nullptr,*brandStatusButton=nullptr,*feedStatusLabel=nullptr,*settingsButton=nullptr,*headerClock=nullptr,*clockButton=nullptr,*headerBattery=nullptr,*deviceBatteryLabel=nullptr,*tabBar=nullptr,*tabs[4]{};
const char*feedStatus="OFFLINE";uint32_t feedStatusColor=LIGHT.amber;
constexpr uint8_t ALL_NODES_PAGE=19;
constexpr uint8_t CLOCK_PAGE=20;
constexpr uint8_t THEME_LIGHT=0,THEME_DARK=1,THEME_AUTO=2;
uint8_t themeMode=THEME_LIGHT;
bool appearanceClockSynced=false;
bool soundEnabled=true;
uint8_t soundVolume=DEFAULT_SOUND_VOLUME;
NotificationPolicy notices;
LowBatteryNoticePolicy batteryNotices;
bool lowBatteryNoticePending=false;
lv_obj_t*notification=nullptr;
uint32_t notificationStarted=0;
uint8_t notificationKind=0;
uint8_t page=ALL_NODES_PAGE,detailOrigin=0,brightness=80,clockZone=0;int selectedGuestId=0;char selectedDiskName[48]{};bool autoRotate=false,idleDim=false,dimmed=false,manualDemo=false,uiReady=false,lightTheme=true;
bool fleetContext=true;
bool utilityPage(uint8_t p){return p==7||p==8||(p>=13&&p<=18)||p==FLIPPER_PAGE;}
bool fleetPage(){return privatePage(page)||page==ALL_NODES_PAGE||page==CLOCK_PAGE||(agentsWorkspace&&(page==OPS_SYSTEMS||(page==OPS_OVERVIEW&&operationsAll())))||(utilityPage(page)&&fleetContext);}
void setViewPage(uint8_t p){if(privatePage(page)&&!privatePage(p)){setPrivateChatActive(false);privateResetUi();}else if(privatePage(p)&&!privatePage(page))setPrivateChatActive(true);fleetContext=privatePage(p)||p==ALL_NODES_PAGE||p==CLOCK_PAGE||(agentsWorkspace&&(p==OPS_SYSTEMS||(p==OPS_OVERVIEW&&operationsAll())))||(utilityPage(p)&&fleetPage());page=p;}
char selectedGpuId[64]{},gpuLinkIds[MAX_GPUS][64]{},nodeLinkIds[MAX_NODES][64]{},trackedNodeId[64]{};uint8_t pickerOrigin=0;uint8_t gpuReturnPage=11,gpuListOrigin=5;
uint32_t lastTouch=0,lastRender=0,lastPoll=0,lastRotate=0,lastNetRevision=0;uint64_t lastSequence=0;
float cpuHistory[32]{},ramHistory[32]{};uint8_t historyCount=0;
ConnectionSettings connectionUi;NodeConfiguration nodeConfigUi;String wifiDraft[5],nodeDraft[7],formMessage,fieldOriginal;ConfigNode editingNode;bool wifiClearPassword=false,nodeExisting=false,nodeClearSecret=false,nodeSavePending=false,nodeDeletePending=false,fieldOriginalToggle=false,wifiSavePending=false;char deleteNodeId[64]{};uint8_t fieldOrigin=14,fieldIndex=0;lv_obj_t*keyboard=nullptr,*fieldArea=nullptr;
const char*const wifiFields[]={"Wi-Fi name","Wi-Fi password","Collector endpoint","Display token","Setup token"};
const char*const nodeFields[]={"Node name","Service URL","Service token","ID (optional slug)","Poll interval (seconds)","Timeout (seconds)","Freshness TTL (seconds)"};
const char*const titles[]={"Overview","Guests","Storage","Sensors"};
void useTheme(bool light){lightTheme=light;const auto&p=light?LIGHT:DARK;BG=p.bg;CARD=p.card;INK=p.ink;MUTED=p.muted;CYAN=p.cyan;LAVENDER=p.lavender;MINT=p.mint;AMBER=p.amber;RED=p.red;EDGE=p.edge;}
bool applyAppearance(){bool light=themeMode==THEME_LIGHT;if(themeMode==THEME_AUTO){struct tm local{};if(!displayClockTm(time(nullptr),clockZone,local))return false;light=local.tm_hour>=7&&local.tm_hour<19;}if(light==lightTheme)return false;useTheme(light);return true;}
void loadAppearance(Preferences&p){clockZone=p.getUChar("clockZone",0)%DISPLAY_CLOCK_ZONE_COUNT;bool savedLight=p.getBool("light",true);themeMode=p.getUChar("themeMode",savedLight?THEME_LIGHT:THEME_DARK);if(themeMode>THEME_AUTO)themeMode=savedLight?THEME_LIGHT:THEME_DARK;useTheme(savedLight);applyAppearance();}
constexpr int TAP_SLOP_PX=12;
lv_obj_t *tapTarget=nullptr;
lv_point_t tapOrigin{};
bool tapDragged=false;
void tapGuard(lv_event_t *e){
  auto code=lv_event_get_code(e);auto *target=lv_event_get_current_target_obj(e);
  if(code==LV_EVENT_DELETE){if(tapTarget==target)tapTarget=nullptr;return;}
  if(code!=LV_EVENT_PRESSED&&code!=LV_EVENT_PRESSING&&code!=LV_EVENT_RELEASED&&code!=LV_EVENT_PRESS_LOST&&code!=LV_EVENT_CLICKED)return;
  auto *input=lv_event_get_indev(e);
  if(!input||lv_indev_get_type(input)!=LV_INDEV_TYPE_POINTER)return;
  lv_point_t point;lv_indev_get_point(input,&point);
  if(code==LV_EVENT_PRESSED){tapTarget=target;tapOrigin=point;tapDragged=consumeWakeTouch;}
  else if(tapTarget==target){
    int32_t dx=point.x-tapOrigin.x,dy=point.y-tapOrigin.y;
    // Retain the maximum excursion, even if the finger returns to its start.
    if(dx*dx+dy*dy>=TAP_SLOP_PX*TAP_SLOP_PX||lv_indev_get_scroll_obj(input)||code==LV_EVENT_PRESS_LOST)tapDragged=true;
  }
  if(code==LV_EVENT_CLICKED&&(tapTarget!=target||tapDragged||consumeWakeTouch))lv_event_stop_processing(e);
}
bool interactionBusy(){
  for(auto *input=lv_indev_get_next(nullptr);input;input=lv_indev_get_next(input)){
    if(lv_indev_get_type(input)==LV_INDEV_TYPE_POINTER&&(lv_indev_get_state(input)==LV_INDEV_STATE_PRESSED||lv_indev_get_scroll_obj(input)))return true;
  }
  return scrollArea&&lv_obj_is_scrolling(scrollArea);
}
lv_color_t color(uint32_t hex){return lv_color_hex(hex);}
float memoryPct(double used,double total){return isfinite(used)&&isfinite(total)&&total>0?used/total*100:NAN;}
String number(double v,int digits=1,const char *suffix=""){return isfinite(v)?String(v,digits)+suffix:String("--");}
String bytes(double v){if(!isfinite(v))return "--";if(v>=1099511627776.0)return String(v/1099511627776.0,1)+" TiB";if(v>=1073741824)return String(v/1073741824,1)+" GiB";if(v>=1048576)return String(v/1048576,1)+" MiB";if(v>=1024)return String(v/1024,0)+" KiB";return String(v,0)+" B";}
String rate(double v){if(!isfinite(v))return "--";if(v>=1e9)return String(v/1e9,1)+" GB/s";if(v>=1e6)return String(v/1e6,1)+" MB/s";if(v>=1e3)return String(v/1e3,0)+" KB/s";return String(v,0)+" B/s";}
String uptime(double v){if(!isfinite(v))return "--";uint32_t t=v;return t>=86400?String(t/86400)+"d "+String((t%86400)/3600)+"h":String(t/3600)+"h "+String((t%3600)/60)+"m";}
String since(double when){if(!isfinite(when))return "--";double seconds=double(time(nullptr))-when;if(seconds<0||time(nullptr)<1700000000)return "Time unavailable";if(seconds<60)return String(int(seconds))+"s ago";if(seconds<3600)return String(int(seconds/60))+"m ago";if(seconds<86400)return String(int(seconds/3600))+"h ago";return String(int(seconds/86400))+"d ago";}
double ageSeconds(){if(!snapshot||!snapshot->valid)return NAN;double age=(millis()-snapshot->received)/1000.0;time_t now=time(nullptr);if(now>1700000000&&isfinite(snapshot->generated))age=max(age,double(now)-snapshot->generated);return max(0.0,age);}
bool stale(){return !snapshot->valid||ageSeconds()>HOMELAB_STALE_MS/1000.0||(deviceNodeType(snapshot->node.type)&&!strcmp(snapshot->node.status,"offline"));}
uint8_t failedSources(){uint8_t failed=0;for(int i=0;i<snapshot->nSources;i++){auto&s=snapshot->sources[i];if(s.enabled&&!s.ok)failed++;}return failed;}

bool demo(){return manualDemo||snapshot->demo;}
bool printerNode(){return !strcmp(snapshot->node.type,"klipper");}
bool deviceNode(){return deviceNodeType(snapshot->node.type);}
constexpr uint8_t SUBNODES_PAGE=21;
uint8_t subnodes(const char*parent,bool healthyOnly=false);
bool hasSubnodes(){return snapshot&&subnodes(snapshot->node.id)>0;}
uint8_t tabPage(uint8_t index){static const uint8_t devicePages[]={0,2,3,6},parentPages[]={0,SUBNODES_PAGE,2,3};return deviceNode()?(hasSubnodes()?parentPages[index%4]:devicePages[index%4]):index%4;}
int pageTab(uint8_t selected){if(!deviceNode())return selected<4?selected:-1;if(hasSubnodes())return selected==0?0:selected==SUBNODES_PAGE?1:selected==2?2:selected==3?3:-1;return selected==0?0:selected==2?1:selected==3?2:selected==6?3:-1;}
bool printerFresh();
lv_obj_t *box(lv_obj_t *parent,int x,int y,int w,int h,uint32_t bg=CARD,int radius=7){auto*o=lv_obj_create(parent);lv_obj_set_pos(o,x,y);lv_obj_set_size(o,w,h);lv_obj_remove_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_SCROLLABLE|LV_OBJ_FLAG_CLICKABLE));lv_obj_set_style_bg_color(o,color(bg),0);lv_obj_set_style_bg_opa(o,LV_OPA_COVER,0);lv_obj_set_style_border_width(o,bg!=BG?1:0,0);lv_obj_set_style_border_color(o,color(EDGE),0);lv_obj_set_style_radius(o,radius,0);lv_obj_set_style_pad_all(o,0,0);return o;}
// Normalize display punctuation only; source text and Unicode paging offsets stay intact.
void normalizeDisplayText(String&text){
  text.replace("\xC2\xB7","•");
  for(const char*mark:{"‘","’","‛","ʼ","＇"})text.replace(mark,"'");
  for(const char*mark:{"“","”","‟"})text.replace(mark,"\"");
  text.replace("–","-");text.replace("—","-");text.replace("…","...");
}
void setDisplayText(lv_obj_t*object,const String&text){String compatible=text;normalizeDisplayText(compatible);lv_label_set_text(object,compatible.c_str());privatechat::wipe(const_cast<char*>(compatible.c_str()),compatible.length());}
void displayTextSize(lv_point_t*size,const String&text,const lv_font_t*font,int32_t letterSpace,int32_t lineSpace,int32_t width,lv_text_flag_t flags){String compatible=text;normalizeDisplayText(compatible);lv_text_get_size(size,compatible.c_str(),font,letterSpace,lineSpace,width,flags);privatechat::wipe(const_cast<char*>(compatible.c_str()),compatible.length());}
lv_obj_t *label(lv_obj_t *parent,const String &text,int x,int y,int w,uint32_t fg=INK,const lv_font_t *font=&lv_font_montserrat_12){
  auto*l=lv_label_create(parent);
  setDisplayText(l,text);
  lv_obj_set_pos(l,x,y);lv_obj_set_width(l,w);lv_obj_set_height(l,lv_font_get_line_height(font));lv_label_set_long_mode(l,LV_LABEL_LONG_DOT);lv_obj_set_style_text_color(l,color(fg),0);lv_obj_set_style_text_font(l,font,0);return l;
}
void clickable(lv_obj_t *o,lv_event_cb_t cb,void *data=nullptr){lv_obj_add_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_CLICKABLE|LV_OBJ_FLAG_PRESS_LOCK));lv_obj_add_event_cb(o,tapGuard,LV_EVENT_ALL,nullptr);lv_obj_add_event_cb(o,cb,LV_EVENT_CLICKED,data);}
void dismissNotification(lv_event_t* =nullptr){if(notification){auto*o=notification;notification=nullptr;lv_obj_delete(o);}}
void styleNotification(){if(!notification)return;uint32_t accent=notificationKind==2?RED:notificationKind==1?AMBER:notificationKind==3?MINT:CYAN;lv_obj_set_style_bg_color(notification,color(CARD),0);lv_obj_set_style_border_color(notification,color(accent),0);lv_obj_set_style_text_color(lv_obj_get_child(notification,0),color(accent),0);lv_obj_set_style_text_color(lv_obj_get_child(notification,1),color(INK),0);lv_obj_set_style_text_color(lv_obj_get_child(notification,2),color(MUTED),0);}
void showNotification(const String&title,const String&message,uint8_t kind=0){
  if(!uiReady)return;dismissNotification();notificationKind=kind;notificationStarted=millis();
  lv_point_t size;displayTextSize(&size,message,&lv_font_montserrat_10,0,0,PAGE_W-20,LV_TEXT_FLAG_NONE);int bodyH=min(26,int(size.y));
  notification=box(lv_screen_active(),8,HEADER_H+4,PAGE_W,38+bodyH,CARD,9);lv_obj_set_style_border_width(notification,2,0);
  label(notification,title,10,8,PAGE_W-40,INK,&lv_font_montserrat_12);auto*body=label(notification,message,10,29,PAGE_W-20,INK,&lv_font_montserrat_10);lv_obj_set_height(body,bodyH);label(notification,LV_SYMBOL_CLOSE,PAGE_W-23,8,14,MUTED);
  clickable(notification,dismissNotification);styleNotification();lv_obj_move_foreground(notification);
}
void render();
void openNodeSettings(lv_event_t*);
void finishField(bool cancel);
void openPage(uint8_t p){if(p==11&&page!=11&&page!=12)gpuListOrigin=page;if(p>=4&&p!=7&&p!=8&&page<4)detailOrigin=page;setViewPage(p);lastRotate=millis();render();}
void tabGo(lv_event_t*e){uint8_t selected=uintptr_t(lv_event_get_user_data(e))%4;if(fleetPage()){if(selected==0){if(agentsWorkspace)operationsHome();else openPage(ALL_NODES_PAGE);}else if(selected==1){if(agentsWorkspace)operationsClockGo();else openPage(CLOCK_PAGE);}else if(selected==2){if(agentsWorkspace)openPage(OPS_SYSTEMS);else{if(page!=13)pickerOrigin=page;openPage(13);}}else openPage(7);return;}if(agentsWorkspace){openPage(OPS_OVERVIEW+selected);return;}openPage(tabPage(selected));}
void resetNodeView(bool retainContext=false){uint8_t previous=page;bool retain=retainContext&&(agentsWorkspace||utilityPage(page)||page==ALL_NODES_PAGE||page==CLOCK_PAGE);bool previousFleet=fleetPage();for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}historyCount=0;lastSequence=0;selectedGuestId=0;selectedDiskName[0]=0;selectedGpuId[0]=0;detailOrigin=0;gpuReturnPage=11;gpuListOrigin=5;page=retain?previous:0;fleetContext=retain&&previousFleet;strlcpy(trackedNodeId,snapshot->node.id,sizeof(trackedNodeId));}
void go(lv_event_t *e){uint8_t selected=uintptr_t(lv_event_get_user_data(e));openPage(printerNode()&&selected==6?3:selected);}
void chooseNode(lv_event_t*){if(agentsWorkspace){openPage(OPS_SYSTEMS);return;}if(page==13)return;pickerOrigin=page;openPage(13);}
void allNodesGo(lv_event_t*){agentsWorkspace=false;savedNodesPage=ALL_NODES_PAGE;Preferences p;if(p.begin("display",false)){p.putBool("agentsMode",false);p.end();}openPage(ALL_NODES_PAGE);}
void workspaceHome(lv_event_t*e){if(agentsWorkspace){savedAgentsPage=OPS_OVERVIEW;operationsHome();}else allNodesGo(e);}
void clockGo(lv_event_t*){if(agentsWorkspace)operationsClockGo();else openPage(CLOCK_PAGE);}
void feedStatusGo(lv_event_t*e){if(privatePage(page)){openPage(7);return;}if(agentsWorkspace){openPage(OPS_SOURCES);return;}if(fleetPage())openPage(7);else go(e);}
void nodeGo(lv_event_t*e){size_t index=uintptr_t(lv_event_get_user_data(e));if(index>=MAX_NODES||!nodeLinkIds[index][0])return;if(manualDemo){int selected=nodeIndexForId(*snapshot,nodeLinkIds[index]);if(selected>=0){snapshot->node=snapshot->nodes[selected];makeDemo(*snapshot);resetNodeView();render();}return;}if(requestNodeSelection(nodeLinkIds[index])){readSnapshot(*snapshot,net);resetNodeView();render();}}

bool applyFlipperNodeSelection(){
  // A rapid local browse sends only the newest absolute node ID. All selection
  // and LVGL work remains on this task, outside the BLE callback.
  static char pendingId[64]{};static uint32_t pendingSequence=0,generation=0;
  flipper::State controls;flipper::readState(controls);
  if(generation!=controls.linkGeneration){generation=controls.linkGeneration;pendingId[0]=0;flipperRequestedNode[0]=0;flipperRequestSequence=0;}
  char nextId[64]{};uint32_t nextSequence=0;
  if(flipper::takeNodeSelection(nextId,sizeof(nextId),nextSequence)){
    strlcpy(pendingId,nextId,sizeof(pendingId));pendingSequence=nextSequence;
  }
  if(!controls.connected||!pendingId[0])return false;
  int selected=nodeIndexForId(*snapshot,pendingId);
  if(selected<0){pendingId[0]=0;return false;}
  const NodeDescriptor chosen=snapshot->nodes[selected];
  const bool changed=strcmp(chosen.id,snapshot->node.id)!=0;
  if(changed&&manualDemo){snapshot->node=chosen;makeDemo(*snapshot);}
  else if(!manualDemo){
    // The UI copy can lag a prior request when its short mutex read failed.
    // Always select through the authoritative model, even for the same UI ID.
    if(!requestNodeSelection(chosen.id))return false;
    if(!readSnapshot(*snapshot,net)||strcmp(snapshot->node.id,chosen.id))return false;
  }
  strlcpy(flipperRequestedNode,chosen.id,sizeof(flipperRequestedNode));
  flipperRequestSequence=pendingSequence;pendingId[0]=0;
  if(changed)resetNodeView(true);
  return true;
}

void guestGo(lv_event_t*e){int index=uintptr_t(lv_event_get_user_data(e));if(index<snapshot->nGuests){selectedGuestId=snapshot->guests[index].id;openPage(4);}}
void back(lv_event_t*){if(privatePage(page)){privateBack();return;}if(operationsPage(page)){operationsBack();return;}if(page==18){finishField(true);return;}openPage(page==14||page==15||page==FLIPPER_PAGE?7:page==16?15:page==17?16:page==13?pickerOrigin:page==4?1:page==9?2:page==11?gpuListOrigin:page==12?gpuReturnPage:detailOrigin);}
void diskGo(lv_event_t*e){int index=uintptr_t(lv_event_get_user_data(e));if(index<snapshot->nDisks){strlcpy(selectedDiskName,snapshot->disks[index].name,sizeof(selectedDiskName));openPage(9);}}
void gpuGo(lv_event_t*e){size_t index=uintptr_t(lv_event_get_user_data(e));if(index<MAX_GPUS&&gpuLinkIds[index][0]){strlcpy(selectedGpuId,gpuLinkIds[index],sizeof(selectedGpuId));gpuReturnPage=page==4?4:11;openPage(12);}}
void gpuIdentities(){for(size_t i=0;i<MAX_GPUS;i++)strlcpy(gpuLinkIds[i],i<snapshot->nGpus?snapshot->gpus[i].id:"",sizeof(gpuLinkIds[i]));}
const lv_font_t*fitFont(const String&text,int width,const lv_font_t*preferred=&lv_font_montserrat_12);
void smallButton(lv_obj_t*parent,const String&t,int x,int y,int w,uint32_t fg,lv_event_cb_t cb,void*user=nullptr){auto*b=box(parent,x,y,w,40,CARD,7);lv_obj_set_style_border_color(b,color(fg),0);label(b,t,4,12,w-8,fg,fitFont(t,w-8));clickable(b,cb,user);}
void progress(lv_obj_t *parent,int x,int y,int w,float v,uint32_t fg){auto*b=lv_bar_create(parent);lv_obj_remove_flag(b,LV_OBJ_FLAG_CLICKABLE);lv_obj_set_pos(b,x,y);lv_obj_set_size(b,w,4);lv_obj_set_style_bg_color(b,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(b,color(fg),LV_PART_INDICATOR);lv_obj_set_style_radius(b,3,LV_PART_MAIN);lv_obj_set_style_radius(b,3,LV_PART_INDICATOR);if(!isfinite(v))lv_obj_set_style_opa(b,LV_OPA_30,0);lv_bar_set_value(b,isfinite(v)?constrain(int(v),0,100):0,LV_ANIM_OFF);}
// Icons and history use bounded native draw tasks, with no bitmap buffers.
void drawIcon(lv_event_t*e){
  auto*o=lv_event_get_current_target_obj(e);auto*layer=lv_event_get_layer(e);lv_area_t a;lv_obj_get_coords(o,&a);int w=lv_area_get_width(&a),h=lv_area_get_height(&a);auto fg=lv_obj_get_style_text_color(o,0);
  auto X=[&](int v){return a.x1+v*(w-1)/24;};auto Y=[&](int v){return a.y1+v*(h-1)/24;};
  auto line=[&](int x1,int y1,int x2,int y2){lv_draw_line_dsc_t d;lv_draw_line_dsc_init(&d);d.color=fg;d.width=1;d.round_start=d.round_end=true;d.p1={X(x1),Y(y1)};d.p2={X(x2),Y(y2)};lv_draw_line(layer,&d);};
  auto rect=[&](int x,int y,int rw,int rh,int r){lv_draw_rect_dsc_t d;lv_draw_rect_dsc_init(&d);d.bg_opa=LV_OPA_TRANSP;d.border_color=fg;d.border_width=1;d.radius=r;lv_area_t b={X(x),Y(y),X(x+rw),Y(y+rh)};lv_draw_rect(layer,&d,&b);};
  auto arc=[&](int x,int y,int r,int start,int end){lv_draw_arc_dsc_t d;lv_draw_arc_dsc_init(&d);d.color=fg;d.width=1;d.center={X(x),Y(y)};d.radius=max(1,r*w/24);d.start_angle=start;d.end_angle=end;lv_draw_arc(layer,&d);};
  uint8_t kind=uintptr_t(lv_event_get_user_data(e));if(kind>=16){int tab=kind-16;kind=fleetPage()?(tab==0?0:tab==1?8:tab==3?7:1):agentsWorkspace?(tab==0?0:tab==1?9:tab==2?10:8):printerNode()?(tab==2?3:tab==3?6:tab):deviceNode()?(tab==1?2:tab==2?3:tab==3?6:0):tab;}switch(kind){
    case 0:rect(3,3,7,7,1);rect(14,3,7,7,1);rect(3,14,7,7,1);rect(14,14,7,7,1);break;
    case 1:rect(3,3,18,7,1);rect(3,14,18,7,1);line(7,6,8,6);line(12,6,17,6);line(7,17,8,17);line(12,17,17,17);break;
    case 2:rect(4,3,16,18,2);arc(12,11,4,0,359);line(12,11,16,15);line(7,18,9,18);break;
    case 3:line(9,5,9,14);line(15,5,15,14);arc(12,5,3,180,359);arc(12,17,5,330,210);line(12,8,12,17);line(18,6,21,6);line(18,10,20,10);break;
    case 4:line(13,2,5,14);line(5,14,12,14);line(12,14,11,22);line(11,22,19,10);line(19,10,12,10);line(12,10,13,2);break;
    case 6:line(4,3,12,1);line(12,1,20,3);line(20,3,19,14);line(19,14,12,22);line(12,22,5,14);line(5,14,4,3);line(8,11,11,14);line(11,14,16,8);break;
    case 5:rect(5,5,14,14,1);rect(9,9,6,6,1);for(int i=7;i<=17;i+=5){line(i,2,i,5);line(i,19,i,22);line(2,i,5,i);line(19,i,22,i);}break;
    case 7:arc(12,12,7,0,359);arc(12,12,3,0,359);for(int i=0;i<4;i++){int x=i==0?12:i==1?21:i==2?12:3,y=i==0?3:i==1?12:i==2?21:12;line(x,y,i==0?12:i==1?18:i==2?12:6,i==0?6:i==1?12:i==2?18:12);}break;
    case 9:arc(12,7,4,0,359);arc(12,23,9,180,359);line(3,21,3,23);line(21,21,21,23);break;
    case 10:rect(5,3,15,18,2);line(8,8,10,10);line(10,10,13,6);line(8,15,16,15);break;
    case 8:arc(12,12,10,0,359);line(12,5,12,12);line(12,12,17,15);break;
  }
}
lv_obj_t*icon(lv_obj_t*p,int kind,int x,int y,int size,uint32_t fg){auto*o=box(p,x,y,size,size,BG,0);lv_obj_set_style_bg_opa(o,LV_OPA_TRANSP,0);lv_obj_set_style_border_width(o,0,0);lv_obj_set_style_text_color(o,color(fg),0);lv_obj_add_event_cb(o,drawIcon,LV_EVENT_DRAW_MAIN,(void*)(uintptr_t)kind);return o;}
// Approved website letterforms, pre-rendered once as small static A8 masks.
// The mark uses Space Grotesk 500 and the wordmark uses 700 with -1.5px tracking.
void drawBrandMask(lv_layer_t*layer,const lv_image_dsc_t&image,int x,int y,uint32_t fg){
  lv_draw_image_dsc_t d;lv_draw_image_dsc_init(&d);d.src=&image;d.recolor=color(fg);d.recolor_opa=LV_OPA_COVER;lv_area_t area={x,y,x+image.header.w-1,y+image.header.h-1};lv_draw_image(layer,&d,&area);
}
void drawBrand(lv_event_t*e){
  auto*o=lv_event_get_current_target_obj(e);lv_area_t a;lv_obj_get_coords(o,&a);drawBrandMask(lv_event_get_layer(e),glimdock::MARK_G,a.x1+glimdock::MARK_G_X,a.y1+glimdock::MARK_G_Y,glimdock::WHITE);
}
void drawWordmark(lv_event_t*e){
  auto*o=lv_event_get_current_target_obj(e);auto*layer=lv_event_get_layer(e);lv_area_t a;lv_obj_get_coords(o,&a);uint32_t ink=lv_color_to_u32(lv_obj_get_style_text_color(o,0));
  drawBrandMask(layer,glimdock::WORDMARK_INK,a.x1+glimdock::WORDMARK_INK_X,a.y1+glimdock::WORDMARK_INK_Y,ink);
  drawBrandMask(layer,glimdock::WORDMARK_DOT,a.x1+glimdock::WORDMARK_DOT_X,a.y1+glimdock::WORDMARK_DOT_Y,feedStatusColor);
}
lv_obj_t*brandMark(lv_obj_t*parent,int x,int y){auto*o=box(parent,x,y,glimdock::MARK_SIZE,glimdock::MARK_SIZE,glimdock::PURPLE,5);lv_obj_set_style_border_width(o,0,0);lv_obj_add_event_cb(o,drawBrand,LV_EVENT_DRAW_MAIN,nullptr);return o;}
lv_obj_t*brandWordmark(lv_obj_t*parent,int x,int y){auto*o=box(parent,x,y,glimdock::WORDMARK_W,glimdock::WORDMARK_H,BG,0);lv_obj_set_style_bg_opa(o,LV_OPA_TRANSP,0);lv_obj_set_style_border_width(o,0,0);lv_obj_set_style_text_color(o,color(lightTheme?glimdock::INK_LIGHT:glimdock::INK_DARK),0);lv_obj_add_event_cb(o,drawWordmark,LV_EVENT_DRAW_MAIN,nullptr);return o;}
void drawHistory(lv_event_t*e){
  auto*o=lv_event_get_current_target_obj(e);auto*layer=lv_event_get_layer(e);auto*values=(const float*)lv_event_get_user_data(e);lv_area_t a;lv_obj_get_coords(o,&a);int w=lv_area_get_width(&a),h=lv_area_get_height(&a);auto fg=lv_obj_get_style_text_color(o,0);
  for(int i=1;i<32;i++)if(isfinite(values[i-1])&&isfinite(values[i])){
    lv_point_precise_t p1={a.x1+(i-1)*(w-1)/31,a.y2-int(constrain(values[i-1],0.0f,100.0f)*(h-2)/100)};
    lv_point_precise_t p2={a.x1+i*(w-1)/31,a.y2-int(constrain(values[i],0.0f,100.0f)*(h-2)/100)};
    lv_draw_triangle_dsc_t fill;lv_draw_triangle_dsc_init(&fill);fill.color=fg;fill.opa=LV_OPA_COVER;fill.grad.dir=LV_GRAD_DIR_VER;fill.grad.stops_count=2;fill.grad.stops[0]={fg,LV_OPA_20,0};fill.grad.stops[1]={fg,LV_OPA_TRANSP,255};
    fill.p[0]=p1;fill.p[1]=p2;fill.p[2]={p2.x,a.y2};lv_draw_triangle(layer,&fill);fill.p[1]=fill.p[2];fill.p[2]={p1.x,a.y2};lv_draw_triangle(layer,&fill);
    lv_draw_line_dsc_t line;lv_draw_line_dsc_init(&line);line.color=fg;line.width=1;line.p1=p1;line.p2=p2;lv_draw_line(layer,&line);
  }
}
void sparkle(lv_obj_t*parent,const float *values,uint32_t fg){auto*c=box(parent,PORTRAIT?118:1,PORTRAIT?35:65,PORTRAIT?96:147,PORTRAIT?22:18,BG,0);lv_obj_set_style_bg_opa(c,LV_OPA_TRANSP,0);lv_obj_set_style_border_width(c,0,0);lv_obj_set_style_text_color(c,color(fg),0);lv_obj_add_event_cb(c,drawHistory,LV_EVENT_DRAW_MAIN,(void*)values);}
lv_obj_t *scroll(int y=6){auto*o=box(content,8,PORTRAIT&&y==28?40:y,PAGE_W,CONTENT_H-(PORTRAIT&&y==28?40:y),BG,0);lv_obj_add_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_SCROLLABLE|LV_OBJ_FLAG_CLICKABLE|LV_OBJ_FLAG_PRESS_LOCK));lv_obj_set_scroll_dir(o,LV_DIR_VER);lv_obj_set_scrollbar_mode(o,LV_SCROLLBAR_MODE_AUTO);lv_obj_set_style_pad_bottom(o,8,0);lv_obj_set_style_bg_color(o,color(EDGE),LV_PART_SCROLLBAR);scrollArea=o;return o;}
void row(lv_obj_t*parent,int y,const String&key,const String&value,uint32_t fg=INK){
  auto*c=box(parent,0,y,LIST_W,38,CARD,9);
  label(c,key,10,PORTRAIT?5:12,PORTRAIT?LIST_W-20:166,MUTED,PORTRAIT?&lv_font_montserrat_10:&lv_font_montserrat_12);
  auto*l=label(c,value,PORTRAIT?10:156,PORTRAIT?21:11,PORTRAIT?LIST_W-20:130,fg,fitFont(value,PORTRAIT?LIST_W-20:130));
  lv_obj_set_style_text_align(l,PORTRAIT?LV_TEXT_ALIGN_LEFT:LV_TEXT_ALIGN_RIGHT,0);
}

void heading(const String&title,const String&right="",bool withBack=false){
  int x=withBack?35:8;if(withBack){auto*b=box(content,0,0,40,40,BG,0);label(b,LV_SYMBOL_LEFT,10,8,18,CYAN);clickable(b,back);}
  label(content,title,x,7,PORTRAIT?VIEW_W-x-8:right.length()?196:PAGE_W-x,INK,&lv_font_montserrat_12);
  if(right.length()){auto*l=label(content,right,PORTRAIT?8:211,PORTRAIT?25:8,PORTRAIT?PAGE_W:101,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);}
}
const lv_font_t*fitFont(const String&text,int width,const lv_font_t*preferred){lv_point_t size;const lv_font_t*fonts[]={preferred,&lv_font_montserrat_10,&lv_font_montserrat_8};for(auto*font:fonts){displayTextSize(&size,text,font,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(size.x<=width)return font;}return &lv_font_montserrat_8;}
void section(lv_obj_t*parent,int y,const String&t,uint32_t fg=MUTED){label(parent,t,4,y,LIST_W-4,fg,fitFont(t,LIST_W-4,&lv_font_montserrat_10));}
void cell(lv_obj_t*p,int index,int top,const String&key,const String&value,uint32_t fg=INK){
  int w=PORTRAIT?LIST_W:147;auto*c=box(p,PORTRAIT?0:(index%2)*152,top+(index/CELL_COLUMNS)*45,w,40,CARD,6);
  label(c,key,7,6,w-14,MUTED,&lv_font_montserrat_10);label(c,value,7,21,w-14,fg,fitFont(value,w-14));
}
int note(lv_obj_t*p,int y,const String&text,uint32_t fg=MUTED){lv_point_t size;displayTextSize(&size,text,&lv_font_montserrat_10,0,0,(LIST_W-14),LV_TEXT_FLAG_NONE);auto*l=label(p,text,7,y,(LIST_W-14),fg,&lv_font_montserrat_10);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);return y+size.y+8;}
double sampleAge(double age,double updated){double elapsed=(millis()-snapshot->received)/1000.0;if(isfinite(age))return max(0.0,age+elapsed);time_t now=time(nullptr);return isfinite(updated)&&now>1700000000?max(0.0,double(now)-updated):NAN;}
bool guestMac(const Guest&g){return !strcmp(g.memoryBasis,"guest-os-macos");}
bool guestOs(const Guest&g){return !strcmp(g.memoryBasis,"guest-os")||!strcmp(g.memoryBasis,"guest-os-arc")||guestMac(g);}
const char*memoryPressureText(const char*p){return !strcmp(p,"normal")?"Normal":!strcmp(p,"warning")?"Elevated":!strcmp(p,"critical")?"Critical":"Unavailable";}
uint32_t memoryPressureColor(const char*p){return !strcmp(p,"normal")?MINT:!strcmp(p,"warning")?AMBER:!strcmp(p,"critical")?RED:MUTED;}
bool guestArc(const Guest&g){return !strcmp(g.memoryBasis,"guest-os-arc");}
String guestAge(const Guest&g){double age=sampleAge(g.memAge,g.memUpdated);return isfinite(age)?number(age,0,"s ago"):String("age unknown");}
double guestPrimary(const Guest&g){return guestArc(g)?g.noncache:g.used;}
void metricValue(lv_obj_t*p,double value,int digits,uint32_t fg){String text=number(value,digits);label(p,text,8,22,128,fg,&lv_font_montserrat_32);lv_point_t size;displayTextSize(&size,text,&lv_font_montserrat_32,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);label(p,"%",min(130,8+size.x+3),37,18,fg,&lv_font_montserrat_14);}
uint32_t batteryColor(BatteryLevel level){return level==BatteryLevel::VeryLow?RED:level==BatteryLevel::Low?AMBER:MUTED;}
const char*batterySymbol(BatteryLevel level){return level==BatteryLevel::High?LV_SYMBOL_BATTERY_3:level==BatteryLevel::Medium?LV_SYMBOL_BATTERY_2:level==BatteryLevel::Low?LV_SYMBOL_BATTERY_1:LV_SYMBOL_BATTERY_EMPTY;}
String batteryVoltage(const BoardBatteryStatus&battery){return battery.available()?String("~")+number(battery.volts,1," V"):String("-- V");}
void updateBatteryLabels(){
  auto battery=boardBattery();uint32_t fg=batteryColor(battery.level);
  if(headerBattery){lv_label_set_text(headerBattery,(String(batterySymbol(battery.level))+" "+batteryVoltage(battery)).c_str());lv_obj_set_style_text_color(headerBattery,color(fg),0);}
  if(deviceBatteryLabel){lv_label_set_text(deviceBatteryLabel,(String(batteryLevelName(battery.level))+" • "+batteryVoltage(battery)).c_str());lv_obj_set_style_text_color(deviceBatteryLabel,color(battery.available()?fg:AMBER),0);}
}
void header(){
  bool headerStale=stale()||(printerNode()&&!printerFresh());bool offline=!net.wifi||(snapshot->valid&&!strcmp(snapshot->node.status,"offline"));
  lv_obj_set_style_bg_color(lv_screen_active(),color(BG),0);lv_obj_set_style_bg_color(content,color(BG),0);
  lv_obj_set_style_text_color(headerTitle,color(lightTheme?glimdock::INK_LIGHT:glimdock::INK_DARK),0);lv_obj_set_style_bg_color(headerMark,color(glimdock::PURPLE),0);lv_obj_set_style_text_color(headerHost,color(MUTED),0);
  lv_obj_set_style_bg_color(headerLine,color(EDGE),0);
  lv_obj_set_style_bg_color(nodeButton,color(CARD),0);lv_obj_set_style_border_color(nodeButton,color(EDGE),0);lv_obj_set_style_text_color(lv_obj_get_child(nodeButton,1),color(CYAN),0);
  lv_obj_set_style_bg_color(settingsButton,color(BG),0);lv_obj_set_style_text_color(lv_obj_get_child(settingsButton,0),color(MUTED),0);
  if(fleetPage()){headerStale=!snapshot->valid||(millis()-snapshot->received)>HOMELAB_STALE_MS;offline=!net.wifi;}
  String host=fleetPage()?String("All systems"):snapshot->node.name[0]?String(snapshot->node.name):snapshot->valid?String(snapshot->host):String("Choose node");setDisplayText(headerHost,host);lv_obj_set_style_text_font(headerHost,fitFont(host,lv_obj_get_width(headerHost),PORTRAIT?&lv_font_montserrat_12:&lv_font_montserrat_10),0);
  struct tm clockTime{};char clockText[8]="--:--";if(displayClockTm(time(nullptr),clockZone,clockTime))strftime(clockText,sizeof(clockText),"%H:%M",&clockTime);lv_label_set_text(headerClock,clockText);lv_obj_set_style_text_color(headerClock,color(page==CLOCK_PAGE?CYAN:INK),0);lv_obj_set_style_bg_color(clockButton,color(BG),0);
  updateBatteryLabels();
  uint32_t previousStatusColor=feedStatusColor;feedStatusColor=demo()?LAVENDER:offline?RED:(!fleetPage()&&net.loading)||headerStale?AMBER:(lightTheme?glimdock::PURPLE:LAVENDER);
  if(feedStatusColor!=previousStatusColor)lv_obj_invalidate(headerTitle);
  feedStatus=demo()?"DEMO":fleetPage()?(offline?"OFFLINE":!snapshot->valid?net.setup?"SETUP":"LOADING":headerStale?"STALE":"LIVE"):net.loading?(net.wifi?"LOADING":"OFFLINE"):snapshot->valid?(!strcmp(snapshot->node.status,"offline")?"OFFLINE":headerStale?"STALE":"LIVE"):net.setup?"SETUP":"OFFLINE";
  if(feedStatusLabel){lv_label_set_text(feedStatusLabel,feedStatus);lv_obj_set_style_text_color(feedStatusLabel,color(feedStatusColor),0);}
  if(agentsWorkspace)operationsHeader();if(privatePage(page))privateHeader();

  lv_obj_set_style_bg_color(tabBar,color(CARD),0);lv_obj_set_style_border_color(tabBar,color(EDGE),0);
  int active=page<4?page:page==4?1:page==9?2:page==12&&gpuReturnPage==4?1:page==5||page==6||page==10||page==11||page==12?detailOrigin:-1;
  const char*const printerTitles[]={"Overview","Job","Temps","Health"};const char*const deviceTitles[]={"Overview","Storage","Sensors","Health"};const char*const parentTitles[]={"Overview","Subnodes","Storage","Sensors"};const char*const allTitles[]={"All systems","Clock","Pick system","Settings"};if(deviceNode())active=page==9?1:pageTab(page)>=0?pageTab(page):pageTab(detailOrigin);if(fleetPage())active=page==CLOCK_PAGE?1:(page==13||page==OPS_SYSTEMS)?2:(utilityPage(page)||privatePage(page))?3:0;
  const char*const operationsTitles[]={"Overview","Agents","Tasks","Events"};if(agentsWorkspace&&!fleetPage())active=operationsTab();
  for(int i=0;i<4;i++){lv_label_set_text(lv_obj_get_child(tabs[i],1),fleetPage()?allTitles[i]:agentsWorkspace?operationsTitles[i]:printerNode()?printerTitles[i]:deviceNode()?(hasSubnodes()?parentTitles[i]:deviceTitles[i]):titles[i]);lv_obj_set_style_text_font(lv_obj_get_child(tabs[i],1),fleetPage()&&PORTRAIT?&lv_font_montserrat_8:&lv_font_montserrat_10,0);lv_obj_set_style_bg_color(tabs[i],color(i==active?(lightTheme?BG:DARK_SELECTED):CARD),0);for(int j=0;j<2;j++)lv_obj_set_style_text_color(lv_obj_get_child(tabs[i],j),color(i==active?CYAN:MUTED),0);auto*rule=lv_obj_get_child(tabs[i],2);lv_obj_set_style_bg_color(rule,color(CYAN),0);if(i==active)lv_obj_remove_flag(rule,LV_OBJ_FLAG_HIDDEN);else lv_obj_add_flag(rule,LV_OBJ_FLAG_HIDDEN);}
}
String span(double value){if(!isfinite(value)||value<0)return "--";uint32_t t=value;return t>=86400?String(t/86400)+"d "+String(t%86400/3600)+"h":t>=3600?String(t/3600)+"h "+String(t%3600/60)+"m":t>=60?String(t/60)+"m":String(t)+"s";}
double printerAge(){return sampleAge(snapshot->printer.age,snapshot->printer.updated);}
bool printerFresh(){double age=printerAge();return snapshot->valid&&!stale()&&isfinite(age)&&age<=snapshot->printer.ttl&&failedSources()==0&&!snapshot->printer.error[0]&&strcmp(snapshot->node.status,"offline")!=0;}
String printerState(){auto&p=snapshot->printer;if(!strcmp(p.klippyState,"disconnected"))return "Disconnected";if(!strcmp(snapshot->node.status,"offline"))return "Offline";if(!strcmp(p.klippyState,"startup"))return "Starting";if(!strcmp(p.klippyState,"shutdown"))return "Klipper shutdown";if(!strcmp(p.klippyState,"error"))return "Klipper error";if(!strcmp(p.state,"printing"))return "Printing";if(!strcmp(p.state,"paused"))return "Paused";if(!strcmp(p.state,"complete"))return "Complete";if(!strcmp(p.state,"cancelled"))return "Cancelled";if(!strcmp(p.state,"error"))return "Job error";if(!strcmp(p.state,"standby")||!strcmp(p.state,"idle")||!strcmp(p.state,"ready"))return "Ready";return p.state[0]?String(p.state):String("Unavailable");}
uint32_t printerStateColor(){auto&p=snapshot->printer;if(!strcmp(p.klippyState,"disconnected"))return AMBER;if(!strcmp(snapshot->node.status,"offline")||!strcmp(p.klippyState,"shutdown")||!strcmp(p.klippyState,"error")||!strcmp(p.state,"error"))return RED;if(!printerFresh()||!strcmp(p.state,"paused")||!strcmp(p.klippyState,"startup"))return AMBER;return !strcmp(p.state,"printing")?MINT:MUTED;}
bool estimatingPrint(){return printerFresh()&&!strcmp(snapshot->printer.state,"printing")&&!strcmp(snapshot->printer.klippyState,"ready");}
String printRemaining(){if(!strcmp(snapshot->printer.state,"paused"))return "Paused";return estimatingPrint()?span(snapshot->printer.remaining):String("--");}
String printEta(){if(!estimatingPrint()||!isfinite(snapshot->printer.eta))return "--";time_t finish=snapshot->printer.eta;struct tm*utc=gmtime(&finish);char text[24];if(!utc)return "--";strftime(text,sizeof(text),"%H:%M UTC",utc);return text;}
const Heater*primaryHeater(bool bed){auto&p=snapshot->printer;for(int i=0;i<p.nHeaters;i++){const char*n=p.heaters[i].name;if(bed?(!strcasecmp(n,"Bed")||strstr(n,"heater_bed")):(!strcasecmp(n,"Nozzle")||strstr(n,"extruder")))return &p.heaters[i];}return nullptr;}
void printerOverview(){
  auto&p=snapshot->printer;bool fresh=printerFresh();uint32_t fg=printerStateColor();auto*r=box(content,8,6,PAGE_W,18,BG,0);auto*dot=box(r,1,7,4,4,fg,2);lv_obj_set_style_border_width(dot,0,0);
  String status=(fresh||!strcmp(p.klippyState,"startup")||!strcmp(p.klippyState,"shutdown")||!strcmp(p.klippyState,"disconnected")||!strcmp(snapshot->node.status,"offline")?printerState():String("Stale printer data"))+(snapshot->nAlerts?" • "+String(snapshot->nAlerts)+" alerts":"");label(r,status,10,3,PAGE_W-76,fg,fitFont(status,PAGE_W-76,&lv_font_montserrat_10));auto*age=label(r,number(printerAge(),0,"s ago  >"),PAGE_W-66,3,66,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(age,LV_TEXT_ALIGN_RIGHT,0);clickable(r,go,(void*)3);
  auto*hero=box(content,8,29,PAGE_W,PORTRAIT?80:64,CARD,9);label(hero,printerState(),10,8,PORTRAIT?120:190,INK,&lv_font_montserrat_14);String progressText=number(fresh?p.progress:NAN,1,"%");lv_point_t progressSize;lv_text_get_size(&progressSize,progressText.c_str(),&lv_font_montserrat_28,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);auto*v=label(hero,progressText,PAGE_W-97,8,86,CYAN,progressSize.x<=86?&lv_font_montserrat_28:&lv_font_montserrat_18);lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);progress(hero,10,PORTRAIT?48:38,PAGE_W-20,fresh?p.progress:NAN,CYAN);label(hero,p.filename[0]?String(p.filename):String("No active file"),10,PORTRAIT?64:51,PAGE_W-20,MUTED,&lv_font_montserrat_8);clickable(hero,go,(void*)1);
  auto*estimate=box(content,8,PORTRAIT?115:97,PAGE_W,17,BG,0);label(estimate,"Est. remaining",0,3,PORTRAIT?110:142,MUTED,&lv_font_montserrat_10);v=label(estimate,printRemaining(),PORTRAIT?112:170,1,PORTRAIT?PAGE_W-112:134,INK,&lv_font_montserrat_12);lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);clickable(estimate,go,(void*)1);
  int heaterW=(PAGE_W-6)/2;for(int i=0;i<2;i++){const Heater*h=primaryHeater(i==1);auto*c=box(content,8+i*(heaterW+6),PORTRAIT?139:117,heaterW,PORTRAIT?62:39,CARD,7);label(c,i?"Bed":"Nozzle",8,5,heaterW-16,MUTED,&lv_font_montserrat_8);label(c,number(fresh&&h?h->temp:NAN,1),8,17,PORTRAIT?heaterW-16:67,INK,&lv_font_montserrat_14);label(c,"/ "+number(fresh&&h?h->target:NAN,0)+" °C",PORTRAIT?8:69,PORTRAIT?42:21,PORTRAIT?heaterW-16:72,MUTED,&lv_font_montserrat_10);clickable(c,go,(void*)2);}
}
void printerJob(){
  auto&p=snapshot->printer;bool fresh=printerFresh();heading("Current job",printerState());auto*s=scroll(28);int y=note(s,0,p.filename[0]?String(p.filename):String("No active file"),INK);cell(s,0,y,"Progress",number(fresh?p.progress:NAN,1,"%"),CYAN);cell(s,1,y,"Layer / total",number(fresh?p.currentLayer:NAN,0)+" / "+number(fresh?p.totalLayers:NAN,0));cell(s,2,y,"Printing time",span(fresh?p.printDuration:NAN));cell(s,3,y,"Total job time",span(fresh?p.totalDuration:NAN));cell(s,4,y,"Est. remaining",printRemaining(),LAVENDER);cell(s,5,y,"Est. finish (UTC)",printEta(),LAVENDER);cell(s,6,y,"Filament used",number(fresh?p.filament:NAN,0," mm"));cell(s,7,y,"Slicer estimate",span(p.slicerTime));cell(s,8,y,"Cooling fan",number(fresh?p.fan:NAN,0,"%"));cell(s,9,y,"Filament sensor",!fresh||p.filamentDetected<0?String("--"):p.filamentDetected?String("Detected"):String("Not detected"));y+=cellBottom(10)+8; y=note(s,y,"Progress source: "+String(p.progressBasis[0]?p.progressBasis:"unavailable"));y=note(s,y,"Remaining estimate: "+String(p.etaBasis[0]?p.etaBasis:"unavailable"));if(p.message[0])y=note(s,y,p.message);if(p.error[0])note(s,y,p.error,RED);
}
String temperatureName(const char*name){if(!strcmp(name,"mcu_temp"))return "MCU temperature";if(!strcmp(name,"raspberry_pi"))return "Raspberry Pi";String result=name;result.replace("temperature_sensor ","");result.replace("temperature_host ","");result.replace("_"," ");return result;}
void printerTemps(){
  auto&p=snapshot->printer;bool fresh=printerFresh();heading("Temperatures",String(p.nHeaters+p.nTemps)+" readings");auto*s=scroll(28);int y=0;if(!p.nHeaters&&!p.nTemps)y=note(s,y,"Printer temperature readings are unavailable.",AMBER);
  for(int i=0;i<p.nHeaters;i++){auto&h=p.heaters[i];auto*c=box(s,0,y,LIST_W,PORTRAIT?78:58,CARD,7);label(c,h.name,8,8,PORTRAIT?LIST_W-16:171,INK,&lv_font_montserrat_12);auto*v=label(c,number(fresh?h.temp:NAN,1," °C"),PORTRAIT?8:180,PORTRAIT?27:7,PORTRAIT?LIST_W-16:108,AMBER,&lv_font_montserrat_18);lv_obj_set_style_text_align(v,PORTRAIT?LV_TEXT_ALIGN_LEFT:LV_TEXT_ALIGN_RIGHT,0);String command="Target "+number(fresh?h.target:NAN,0," °C")+" • Duty "+number(fresh?h.duty:NAN,1,"%");label(c,command,8,PORTRAIT?57:34,LIST_W-16,MUTED,fitFont(command,LIST_W-16,&lv_font_montserrat_10));y+=PORTRAIT?84:64;}
  for(int i=0;i<p.nTemps;i++){row(s,y,temperatureName(p.temperatures[i].name),number(fresh?p.temperatures[i].temp:NAN,1," °C"),AMBER);y+=44;}
  row(s,y,"Cooling fan",number(fresh?p.fan:NAN,0,"%"));y+=46;note(s,y,"Heater duty is commanded PWM %, not measured watts.");
}
void printerHealth(){
  auto&p=snapshot->printer;heading("Printer health",snapshot->node.status);auto*s=scroll(28);int y=0;row(s,y,"Klipper",p.klippyState[0]?String(p.klippyState):String("Unavailable"),printerStateColor());y+=44;row(s,y,"Job state",printerState(),printerStateColor());y+=44;row(s,y,"Sample age",number(printerAge(),0,"s"),printerFresh()?MINT:AMBER);y+=44;if(p.message[0])y=note(s,y,p.message);if(p.error[0])y=note(s,y,p.error,RED);
  if(!snapshot->nAlerts){row(s,y,"Active alerts","None",MINT);y+=44;}for(int i=0;i<snapshot->nAlerts;i++){auto&a=snapshot->alerts[i];section(s,y,a.severity,!strcmp(a.severity,"critical")?RED:AMBER);y=note(s,y+18,a.message,INK);}
  y=note(s,y,"Node: "+String(snapshot->node.name));y=note(s,y,"Address: "+String(snapshot->node.address));if(p.hostName[0])y=note(s,y,"Host: "+String(p.hostName));section(s,y,"PRINTER SOURCES");y+=20;for(int i=0;i<snapshot->nSources;i++){auto&source=snapshot->sources[i];row(s,y,source.name,!source.enabled?String("Disabled"):source.ok?String("OK • ")+number(sampleAge(source.age,source.updated),0,"s"):String("Unavailable"),!source.enabled?MUTED:source.ok?MINT:AMBER);y+=44;if(source.error[0])y=note(s,y,source.error,AMBER);}if(!snapshot->nSources)note(s,y,"Printer source status unavailable.",AMBER);
}
bool nodeSummaryFresh(const NodeDescriptor&node){const auto&s=node.summary;double age=sampleAge(s.age,s.generated);return s.available&&snapshot->valid&&(net.wifi||demo())&&(millis()-snapshot->received)<=HOMELAB_STALE_MS&&strcmp(node.status,"offline")&&strcmp(s.status,"offline")&&(!isfinite(age)||age<=s.ttl);}
const char*nodeSummaryState(const NodeDescriptor&node){if(!snapshot->valid||(!net.wifi&&!demo())||!strcmp(node.status,"offline")||!strcmp(node.summary.status,"offline"))return "Offline";if((millis()-snapshot->received)>HOMELAB_STALE_MS||(node.summary.available&&isfinite(sampleAge(node.summary.age,node.summary.generated))&&sampleAge(node.summary.age,node.summary.generated)>node.summary.ttl))return "Stale";const char*status=node.summary.status[0]?node.summary.status:node.status;return !strcmp(status,"healthy")?"Healthy":!strcmp(status,"degraded")?"Degraded":!strcmp(status,"applying")?"Applying":"Unknown";}
bool rootNode(const NodeDescriptor&node){return !node.parentId[0];}
uint8_t rootNodes(){uint8_t count=0;for(int i=0;i<snapshot->nNodes;i++)if(rootNode(snapshot->nodes[i]))count++;return count;}
uint8_t subnodes(const char*parent,bool healthyOnly){uint8_t count=0;for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(!strcmp(n.parentId,parent)&&(!healthyOnly||(nodeSummaryFresh(n)&&!strcmp(nodeSummaryState(n),"Healthy"))))count++;}return count;}
const char*fleetNodeState(const NodeDescriptor&node){const char*state=nodeSummaryState(node);return !strcmp(state,"Healthy")&&subnodes(node.id)!=subnodes(node.id,true)?"Degraded":state;}
void fillNodeLinks(){for(int i=0;i<MAX_NODES;i++)strlcpy(nodeLinkIds[i],i<snapshot->nNodes?snapshot->nodes[i].id:"",sizeof(nodeLinkIds[i]));}
uint8_t healthyNodes(){uint8_t count=0;for(int i=0;i<snapshot->nNodes;i++)if(rootNode(snapshot->nodes[i])&&nodeSummaryFresh(snapshot->nodes[i])&&!strcmp(fleetNodeState(snapshot->nodes[i]),"Healthy"))count++;return count;}
String healthySummary(){return String(healthyNodes())+" / "+String(rootNodes())+" healthy";}
String summaryPrimary(const NodeDescriptor&n){const auto&v=n.summary;bool fresh=nodeSummaryFresh(n);if(!strcmp(n.type,"klipper"))return fresh?String(v.printState[0]?v.printState:"Job")+" "+number(v.progress,0,"%"):String("Progress --");return "CPU "+number(fresh?v.cpu:NAN,0,"%")+" • RAM "+number(fresh?v.memory:NAN,0,"%");}
lv_obj_t*overviewMetricCard(int x,int y,int w,int h,const String&key,const String&value,uint32_t fg,uint8_t target,bool inlineValue=false){
  auto*c=box(content,x,y,w,h,CARD,9);const auto*captionFont=&lv_font_montserrat_10;const auto*valueFont=&lv_font_montserrat_16;
  int interior=h-2*lv_obj_get_style_border_width(c,0),captionHeight=lv_font_get_line_height(captionFont),valueHeight=lv_font_get_line_height(valueFont);
  int captionY=inlineValue?(interior-captionHeight)/2:(interior-captionHeight-3-valueHeight)/2;
  int valueY=inlineValue?(interior-valueHeight)/2:captionY+captionHeight+3;
  label(c,key,7,captionY,inlineValue?79:w-14,MUTED,captionFont);
  int valueX=inlineValue?91:7,valueW=w-valueX-7;auto*v=label(c,value,valueX,valueY,valueW,fg,valueFont);
  lv_point_t size;lv_text_get_size(&size,value.c_str(),valueFont,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(size.x>valueW)lv_obj_set_style_text_letter_space(v,-1,0);
  if(inlineValue)lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);clickable(c,go,(void*)(uintptr_t)target);return c;
}
double fleetAverage(bool memory){double total=0;int count=0;for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];double value=memory?n.summary.memory:n.summary.cpu;if(rootNode(n)&&strcmp(n.type,"klipper")&&nodeSummaryFresh(n)&&isfinite(value)&&value>=0&&value<=100){total+=value;count++;}}return count?total/count:NAN;}
void clockOverview(){
  struct tm clockTime{};bool synced=displayClockTm(time(nullptr),clockZone,clockTime);char timeText[8]="--:--",dateText[32]="Waiting for time sync";if(synced){strftime(timeText,sizeof(timeText),"%H:%M",&clockTime);strftime(dateText,sizeof(dateText),"%a, %d %b",&clockTime);}
  const int clockW=PORTRAIT?PAGE_W:174,clockH=PORTRAIT?120:118;auto*hero=box(content,8,6,clockW,clockH,CARD,12);lv_obj_set_style_border_width(hero,0,0);
  label(hero,"DESK CLOCK",10,9,clockW-76,MUTED,&lv_font_montserrat_8);auto*z=label(hero,displayClockZoneName(clockZone),clockW-76,9,66,LAVENDER,&lv_font_montserrat_8);lv_obj_set_style_text_align(z,LV_TEXT_ALIGN_RIGHT,0);
  auto*t=label(hero,timeText,8,27,clockW-16,INK,&lv_font_montserrat_48);lv_obj_set_style_text_align(t,LV_TEXT_ALIGN_CENTER,0);lv_obj_set_style_text_letter_space(t,-2,0);
  auto*date=label(hero,dateText,10,82,clockW-20,MUTED,&lv_font_montserrat_12);lv_obj_set_style_text_align(date,LV_TEXT_ALIGN_CENTER,0);
  auto*health=label(hero,agentsWorkspace?operationsClockSummary():snapshot->nNodes?healthySummary():String("No nodes configured"),10,101,clockW-20,agentsWorkspace?(operationsNet.connected?CYAN:AMBER):!snapshot->nNodes?MUTED:healthyNodes()==rootNodes()?MINT:AMBER,&lv_font_montserrat_10);lv_obj_set_style_text_align(health,LV_TEXT_ALIGN_CENTER,0);if(agentsWorkspace){clickable(hero,[](lv_event_t*){operationsHome();});operationsClockExtras(clockW);return;}clickable(hero,allNodesGo);
  for(int i=0;i<2;i++){int w=(clockW-6)/2,valueX=PORTRAIT?66:48,valueW=w-valueX-6;auto*c=box(content,8+i*(w+6),130,w,26,CARD,7);label(c,i?"Avg RAM":"Avg CPU",6,8,43,MUTED,&lv_font_montserrat_8);String value=number(fleetAverage(i),0,"%");auto*v=label(c,value,valueX,6,valueW,i?LAVENDER:CYAN,fitFont(value,valueW,&lv_font_montserrat_12));lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);}
  for(int i=0;i<MAX_NODES;i++)strlcpy(nodeLinkIds[i],i<snapshot->nNodes?snapshot->nodes[i].id:"",sizeof(nodeLinkIds[i]));
  const int x=PORTRAIT?8:188,w=PORTRAIT?(PAGE_W-6)/2:124,top=PORTRAIT?162:6,h=PORTRAIT?24:34,gap=PORTRAIT?3:4;
  int slot=0;for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(!rootNode(n))continue;int position=slot++;const char*state=fleetNodeState(n);bool fresh=nodeSummaryFresh(n);uint32_t fg=!strcmp(state,"Healthy")&&fresh?MINT:!strcmp(state,"Offline")?RED:AMBER;auto*c=box(content,x+(PORTRAIT?(position%2)*(w+6):0),top+(PORTRAIT?position/2:position)*(h+gap),w,h,CARD,7);lv_obj_set_style_border_width(c,0,0);auto*dot=box(c,w-10,7,4,4,fg,2);lv_obj_set_style_border_width(dot,0,0);label(c,n.name,7,PORTRAIT?2:4,w-22,INK,fitFont(n.name,w-22,&lv_font_montserrat_10));String summary=fresh?summaryPrimary(n):String(state)+" • readings --";label(c,summary,7,PORTRAIT?15:21,w-14,fresh?MUTED:fg,fitFont(summary,w-14,&lv_font_montserrat_8));clickable(c,nodeGo,(void*)(uintptr_t)i);}
  if(!snapshot->nNodes){auto*c=box(content,x,top,w,PORTRAIT?50:150,CARD,9);label(c,"No nodes yet",8,12,w-16,MUTED,&lv_font_montserrat_10);label(c,"Add nodes",8,32,w-16,CYAN,&lv_font_montserrat_12);clickable(c,openNodeSettings);}
}
void nodesOverview(){
  heading("All systems",PORTRAIT?healthySummary():String(""));if(!PORTRAIT)label(content,healthySummary(),94,9,88,MUTED,fitFont(healthySummary(),88,&lv_font_montserrat_10));workspaceSwitchControl();
  if(!snapshot->nNodes){auto*s=scroll(PORTRAIT?40:28);int y=note(s,0,net.loading?"Waiting for the collector node list...":"Add nodes to your collector to see their summaries here.");smallButton(s,"Manage collector nodes",0,y,LIST_W,CYAN,openNodeSettings);return;}
  constexpr int gap=6;const int width=(PAGE_W-gap)/2,height=PORTRAIT?83:62,top=PORTRAIT?40:28;
  for(int i=0;i<MAX_NODES;i++)strlcpy(nodeLinkIds[i],i<snapshot->nNodes?snapshot->nodes[i].id:"",sizeof(nodeLinkIds[i]));
  int slot=0;for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(!rootNode(n))continue;int position=slot++;const auto&v=n.summary;bool fresh=nodeSummaryFresh(n);const char*state=fleetNodeState(n);uint32_t stateColor=!strcmp(state,"Healthy")&&fresh?MINT:!strcmp(state,"Offline")?RED:AMBER;auto*c=box(content,8+(position%2)*(width+gap),top+(position/2)*(height+gap),width,height,CARD,9);const lv_font_t*nameFont=PORTRAIT?&lv_font_montserrat_10:&lv_font_montserrat_12;const lv_font_t*metricFont=PORTRAIT?&lv_font_montserrat_8:&lv_font_montserrat_10;
    const int rowGap=PORTRAIT?5:2,nameH=lv_font_get_line_height(nameFont),smallH=lv_font_get_line_height(&lv_font_montserrat_8),metricH=lv_font_get_line_height(metricFont);
    const int blockH=nameH+2*smallH+metricH+3*rowGap,innerH=height-2*lv_obj_get_style_border_width(c,0),nameY=(innerH-blockH)/2,subtitleY=nameY+nameH+rowGap,primaryY=subtitleY+smallH+rowGap,secondaryY=primaryY+metricH+rowGap;
    auto*dot=box(c,width-13,nameY+(nameH-5)/2,5,5,stateColor,3);lv_obj_set_style_border_width(dot,0,0);auto*name=label(c,n.name,8,nameY,width-23,INK,nameFont);lv_point_t nameSize;lv_text_get_size(&nameSize,n.name,nameFont,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(nameSize.x>width-23)lv_obj_set_style_text_letter_space(name,-1,0);String subtitle=String(nodeTypeName(n))+" • "+state;label(c,subtitle,8,subtitleY,width-16,MUTED,&lv_font_montserrat_8);
    String primary,secondary;bool printer=!strcmp(n.type,"klipper");if(printer){primary=fresh?String(v.printState[0]?v.printState:"Job")+" "+number(v.progress,0,"%"):String("Progress --");secondary="Temp "+number(fresh?v.temp:NAN,0," °C");}else{primary="CPU "+number(fresh?v.cpu:NAN,0,"%")+" • RAM "+number(fresh?v.memory:NAN,0,"%");bool proxmox=!strcmp(n.type,"proxmox");int32_t count=proxmox?v.guests:v.sensors;secondary=(fresh&&count>=0?String(count):String("--"))+(proxmox?" guests":" sensors")+" • "+number(fresh?v.temp:NAN,0," °C");}
    uint8_t children=subnodes(n.id);if(children)secondary=String(children)+(children==1?" subnode":" subnodes")+" • "+String(subnodes(n.id,true))+" healthy";
    auto*metric=label(c,primary,8,primaryY,width-16,fresh?CYAN:MUTED,metricFont);lv_point_t metricSize;lv_text_get_size(&metricSize,primary.c_str(),metricFont,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(metricSize.x>width-16)lv_obj_set_style_text_letter_space(metric,-1,0);auto*detail=label(c,secondary,8,secondaryY,width-16,MUTED,&lv_font_montserrat_8);lv_point_t detailSize;lv_text_get_size(&detailSize,secondary.c_str(),&lv_font_montserrat_8,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(detailSize.x>width-16)lv_obj_set_style_text_letter_space(detail,-1,0);clickable(c,nodeGo,(void*)(uintptr_t)i);
  }
}
void nodePicker(){
  heading("Pick system",String(snapshot->nNodes)+" nodes",true);auto*s=scroll(PORTRAIT?40:28);smallButton(s,"All systems • summary",0,0,LIST_W,CYAN,allNodesGo);smallButton(s,"Switch to Agents",0,46,LIST_W,LAVENDER,toggleWorkspace);int y=92;for(int i=0;i<MAX_NODES;i++)strlcpy(nodeLinkIds[i],i<snapshot->nNodes?snapshot->nodes[i].id:"",sizeof(nodeLinkIds[i]));
  for(int i=0;i<snapshot->nNodes;i++){auto&n=snapshot->nodes[i];auto*c=box(s,0,y,LIST_W,62,CARD,7);if(!strcmp(n.id,snapshot->node.id))lv_obj_set_style_border_color(c,color(CYAN),0);label(c,n.name,10,8,LIST_W-99,INK,&lv_font_montserrat_12);auto*state=label(c,n.status,LIST_W-85,10,75,!strcmp(n.status,"healthy")?MINT:AMBER,&lv_font_montserrat_8);lv_obj_set_style_text_align(state,LV_TEXT_ALIGN_RIGHT,0);int parent=nodeIndexForId(*snapshot,n.parentId);int height=note(c,30,parent>=0?String("Subnode of ")+snapshot->nodes[parent].name:String(nodeTypeName(n))+" • "+n.address);lv_obj_set_height(c,max(62,height+4));y+=max(62,height+4)+6;clickable(c,nodeGo,(void*)(uintptr_t)i);}
  if(!snapshot->nNodes)note(s,y,"Connect to a collector feed to discover configured nodes.",MUTED);
}
void overview(){
  uint8_t issues=snapshot->nAlerts;bool incomplete=failedSources()>0||!isfinite(snapshot->cpu)||!isfinite(snapshot->memTotal);bool critical=false;for(int i=0;i<snapshot->nAlerts;i++)if(!strcmp(snapshot->alerts[i].severity,"critical"))critical=true;uint32_t health=stale()?AMBER:issues?(critical?RED:AMBER):incomplete?AMBER:MINT;
  auto*r=box(content,8,PORTRAIT?2:6,PAGE_W,18,BG,0);String healthText=stale()?"Feed disconnected":issues?String(issues)+" alert"+(issues==1?"":"s")+" need attention":incomplete?"Some metrics unavailable":"No active alerts";
  uint8_t bannerPage=6;int parent=nodeIndexForId(*snapshot,snapshot->node.parentId);uint8_t children=subnodes(snapshot->node.id);if(parent>=0){fillNodeLinks();healthText=String("Back to ")+snapshot->nodes[parent].name;}else if(children&&!issues&&!stale()){healthText=String(children)+(children==1?" subnode":" subnodes")+" • "+String(subnodes(snapshot->node.id,true))+" healthy";health=subnodes(snapshot->node.id,true)==children?MINT:AMBER;bannerPage=SUBNODES_PAGE;}
  auto*dot=box(r,1,7,4,4,health,2);lv_obj_set_style_border_width(dot,0,0);label(r,healthText,10,3,PAGE_W-72,health,fitFont(healthText,PAGE_W-72,&lv_font_montserrat_10));auto*a=label(r,stale()?"STALE  >":number(ageSeconds(),0,"s ago  >"),PAGE_W-62,3,62,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(a,LV_TEXT_ALIGN_RIGHT,0);if(parent>=0)clickable(r,nodeGo,(void*)(uintptr_t)parent);else clickable(r,go,(void*)(uintptr_t)bannerPage);
  auto*cpu=box(content,8,PORTRAIT?22:29,PORTRAIT?PAGE_W:149,PORTRAIT?62:84,CARD,9);label(cpu,"CPU",8,7,56,MUTED,&lv_font_montserrat_10);metricValue(cpu,snapshot->cpu,1,CYAN);
  String cpuSummary=(snapshot->nCores?String(snapshot->nCores):snapshot->cpuCount?String(snapshot->cpuCount):String("--"))+(PORTRAIT?" threads\nLoad ":" threads • load ")+number(snapshot->load[0],1);auto*cpuNote=label(cpu,cpuSummary,PORTRAIT?118:8,PORTRAIT?8:55,PORTRAIT?96:135,MUTED,&lv_font_montserrat_10);if(PORTRAIT){lv_label_set_long_mode(cpuNote,LV_LABEL_LONG_WRAP);lv_obj_set_height(cpuNote,27);}sparkle(cpu,cpuHistory,CYAN);clickable(cpu,go,(void*)5);
  auto*ram=box(content,PORTRAIT?8:163,PORTRAIT?89:29,PORTRAIT?PAGE_W:149,PORTRAIT?62:84,CARD,9);label(ram,"MEMORY",8,7,100,MUTED,&lv_font_montserrat_10);metricValue(ram,memoryPct(snapshot->memUsed,snapshot->memTotal),0,LAVENDER);
  String ramSummary=number(snapshot->memUsed/1073741824,1)+" / "+number(snapshot->memTotal/1073741824,1)+" GiB";auto*ramNote=label(ram,ramSummary,PORTRAIT?118:8,PORTRAIT?8:55,PORTRAIT?96:135,MUTED,&lv_font_montserrat_10);if(PORTRAIT){lv_label_set_long_mode(ramNote,LV_LABEL_LONG_WRAP);lv_obj_set_height(ramNote,27);}sparkle(ram,ramHistory,LAVENDER);clickable(ram,go,(void*)10);
  // Secondary metrics share a card grid; the last card rotates in place.
  bool fresh=!stale();const int gap=6,w=PORTRAIT?(PAGE_W-gap)/2:(PAGE_W-2*gap)/3,top=PORTRAIT?156:118,h=PORTRAIT?36:42;
  for(int i=0;i<2;i++){String value=i?number(fresh?snapshot->watts:NAN,0," W"):number(fresh?snapshot->temp:NAN,0," °C");overviewMetricCard(8+i*(w+gap),top,w,h,i?"CPU power":"CPU temp",value,i?MINT:INK,i?5:3);}
  const int phases=snapshot->nGpus?6:5;int phase=(millis()/6000)%phases;String key,value;uint8_t target=10;
  if(phase==0){key="Network in";value=rate(fresh?snapshot->rx:NAN);}
  else if(phase==1){key="Network out";value=rate(fresh?snapshot->tx:NAN);}
  else if(phase==2){if(deviceNode()){if(hasSubnodes()){key="Subnodes";value=String(subnodes(snapshot->node.id,true))+" / "+String(subnodes(snapshot->node.id));target=SUBNODES_PAGE;}else{key="Uptime";value=uptime(fresh?snapshot->uptime:NAN);}}else{key="Guests";int online=0;for(int i=0;i<snapshot->nGuests;i++)if(!strcmp(snapshot->guests[i].status,"running"))online++;value=fresh?String(online)+" / "+String(snapshot->nGuests):String("--");target=1;}}
  else if(phase==3){key="Disk temp";float hottest=NAN;for(int i=0;i<snapshot->nDisks;i++)if(isfinite(snapshot->disks[i].temp))hottest=isfinite(hottest)?max(hottest,snapshot->disks[i].temp):snapshot->disks[i].temp;value=number(fresh?hottest:NAN,0," °C");target=2;}
  else if(phase==4){key="Fan speed";float fan=NAN;for(int i=0;i<snapshot->nSensors;i++){const auto&v=snapshot->sensors[i];if(!strcmp(v.kind,"fan")&&isfinite(v.value))fan=isfinite(fan)?max(fan,v.value):v.value;}value=number(fresh?fan:NAN,0," RPM");target=3;}
  else{int selected=0;for(int i=0;i<snapshot->nGpus;i++)if(!strcmp(snapshot->gpus[i].kind,"discrete")&&isfinite(snapshot->gpus[i].utilization)){selected=i;break;}const auto&g=snapshot->gpus[selected];key=!strcmp(g.utilizationKind,"busiest-engine")?"Engine load":"Graphics";value=number(fresh?g.utilization:NAN,0,"%");target=11;}
  const int cardX=PORTRAIT?8:8+2*(w+gap),cardY=PORTRAIT?198:top,cardW=PORTRAIT?PAGE_W:w,cardH=PORTRAIT?26:h;auto*c=overviewMetricCard(cardX,cardY,cardW,cardH,key,value,CYAN,target,PORTRAIT);for(int i=0;i<phases;i++){auto*d=box(c,cardW-7-phases*4+i*4,cardH-4,2,2,i==phase?CYAN:EDGE,1);lv_obj_set_style_border_width(d,0,0);}

}
void guests(){
  int running=0;for(int i=0;i<snapshot->nGuests;i++)if(!strcmp(snapshot->guests[i].status,"running"))running++;
  heading(PORTRAIT?"Guests":"Virtual machines & containers",String(running)+" running");auto*s=scroll(28);
  if(!snapshot->nGuests){row(s,0,"Guest inventory","Unavailable",AMBER);return;}
  for(int i=0;i<snapshot->nGuests;i++){auto&g=snapshot->guests[i];double primary=guestPrimary(g);auto*c=box(s,0,i*68,LIST_W,63);auto*tag=box(c,7,7,25,14,lightTheme?0xe8eef4:0x26384c,3);lv_obj_set_style_border_width(tag,0,0);label(tag,!strcmp(g.type,"lxc")?"CT":"VM",4,3,22,MUTED,&lv_font_montserrat_8);label(c,g.name,38,7,LIST_W-113,INK,&lv_font_montserrat_12);auto*status=label(c,g.status,LIST_W-72,9,65,!strcmp(g.status,"running")?MINT:MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(status,LV_TEXT_ALIGN_RIGHT,0);
    label(c,String(g.id)+" • CPU "+number(g.cpu,1,"%"),7,28,PORTRAIT?102:128,MUTED,&lv_font_montserrat_10);String ram=String(guestArc(g)?"~":"")+number(primary/1073741824,1)+" / "+number(g.total/1073741824,1)+" GiB  >";auto*mem=label(c,ram,PORTRAIT?111:135,28,PORTRAIT?LIST_W-118:154,LAVENDER,fitFont(ram,PORTRAIT?LIST_W-118:154,&lv_font_montserrat_10));lv_obj_set_style_text_align(mem,LV_TEXT_ALIGN_RIGHT,0);
    String basis=guestArc(g)?"OS/apps ~ • ARC "+bytes(g.cache)+" • "+guestAge(g):guestMac(g)?"macOS RAM • "+String(memoryPressureText(g.memoryPressure))+" • "+guestAge(g):guestOs(g)?"Guest RAM • "+guestAge(g):"PVE RAM • Proxmox accounting";if(!isfinite(primary))basis=(guestArc(g)?String("OS/apps ~ unavailable"):guestOs(g)?String("Guest RAM unavailable"):String("PVE RAM unavailable"))+" • "+guestAge(g);label(c,basis,7,43,(LIST_W-14),isfinite(primary)?MUTED:AMBER,&lv_font_montserrat_8);progress(c,7,56,(LIST_W-14),memoryPct(primary,g.total),strcmp(g.status,"running")?MUTED:LAVENDER);clickable(c,guestGo,(void*)(uintptr_t)i);
  }
}
void guestDetail(){
  int selectedGuest=-1;for(int i=0;i<snapshot->nGuests;i++)if(snapshot->guests[i].id==selectedGuestId)selectedGuest=i;
  if(selectedGuest<0){label(content,"Guest no longer present",12,18,LIST_W,AMBER);return;}
  auto&g=snapshot->guests[selectedGuest];heading(g.name,String(!strcmp(g.type,"lxc")?"CT ":"VM ")+g.id,true);label(content,g.status,8,PORTRAIT?43:29,80,strcmp(g.status,"running")?MUTED:MINT,&lv_font_montserrat_10);auto*up=label(content,"Uptime "+uptime(g.uptime),PORTRAIT?90:114,PORTRAIT?43:29,PORTRAIT?PAGE_W-82:198,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(up,LV_TEXT_ALIGN_RIGHT,0);
  auto*s=scroll(PORTRAIT?62:46);cell(s,0,0,"CPU",number(g.cpu,1,"%"),CYAN);cell(s,1,0,guestArc(g)?"Used incl. ARC":guestOs(g)?"Guest RAM":"PVE RAM",number(g.used/1073741824,1)+" / "+number(g.total/1073741824,1)+" GiB",LAVENDER);cell(s,2,0,"Network in",rate(g.rx));cell(s,3,0,"Network out",rate(g.tx));cell(s,4,0,"Disk read",rate(g.read));cell(s,5,0,"Disk write",rate(g.write));int y=cellBottom(6)+4;
  y=note(s,y,String(guestArc(g)?"Guest OS + ARC":guestOs(g)?"Guest OS":"Proxmox accounting")+" • "+guestAge(g),isfinite(g.used)?MUTED:AMBER);if(g.memError[0])y=note(s,y,g.memError,AMBER);
  int i=0;if(guestMac(g)){cell(s,i++,y,"Memory pressure",memoryPressureText(g.memoryPressure),memoryPressureColor(g.memoryPressure));cell(s,i++,y,"Compressed",bytes(g.compressed),LAVENDER);cell(s,i++,y,"App memory ~",bytes(g.app));cell(s,i++,y,"Wired",bytes(g.wired));cell(s,i++,y,"Cached files",bytes(g.cache));cell(s,i++,y,"Swap used",bytes(g.swapUsed));}cell(s,i++,y,"Available",bytes(g.available));if(guestArc(g)){cell(s,i++,y,"ARC cache",bytes(g.cache),LAVENDER);cell(s,i++,y,"OS/apps ~ estimate",bytes(g.noncache),LAVENDER);}cell(s,i++,y,"Assigned RAM",bytes(g.assigned));cell(s,i++,y,"Host footprint",bytes(g.hostMem));cell(s,i++,y,"Proxmox RAM",bytes(g.pveUsed)+" / "+bytes(g.pveTotal));y+=cellBottom(i)+8;
  if(guestMac(g))y=note(s,y,"Native macOS estimate: app + wired + physical compression. Cached files are reclaimable; pressure is reported by macOS.");
  if(guestArc(g))y=note(s,y,"OS/apps ~ = guest used minus ARC; this is an estimate. ARC cache can be reclaimed.");
  gpuIdentities();String owner="vm:"+String(g.id);bool found=false;for(int k=0;k<snapshot->nGpus;k++)if(owner==snapshot->gpus[k].owner){if(!found){section(s,y,"ASSIGNED GPUS");y+=22;found=true;}smallButton(s,snapshot->gpus[k].name,0,y,LIST_W,CYAN,gpuGo,(void*)(uintptr_t)k);y+=46;}
  note(s,y,"Disk values are throughput, not guest capacity.");
}
void storage(){
  heading("Storage",deviceNode()?"Filesystems & disks":"Pools & disks");auto*s=scroll(28);int y=0;
  if(!snapshot->nPools){row(s,y,deviceNode()?"Filesystems":"Storage pools","Unavailable",AMBER);y+=44;}
  for(int i=0;i<snapshot->nPools;i++){auto&p=snapshot->pools[i];auto*c=box(s,0,y,LIST_W,52);label(c,p.name,7,7,LIST_W-94,INK,&lv_font_montserrat_12);auto*l=label(c,number(memoryPct(p.used,p.total),0,"%"),LIST_W-85,7,78,INK);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,p.status,7,28,104,MUTED,&lv_font_montserrat_10);String capacity=bytes(p.used)+" / "+bytes(p.total);l=label(c,capacity,PORTRAIT?91:111,28,PORTRAIT?LIST_W-98:178,MUTED,fitFont(capacity,PORTRAIT?LIST_W-98:178,&lv_font_montserrat_10));lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);progress(c,7,44,(LIST_W-14),memoryPct(p.used,p.total),memoryPct(p.used,p.total)>85?AMBER:CYAN);y+=57;}
  section(s,y+3,deviceNode()?"DISK INVENTORY":"PHYSICAL DISKS • SMART");y+=23;
  if(!snapshot->nDisks){row(s,y,"Disk telemetry","Unavailable",AMBER);y+=44;}
  for(int i=0;i<snapshot->nDisks;i++){auto&d=snapshot->disks[i];auto*c=box(s,0,y,LIST_W,55);label(c,d.name,7,7,LIST_W-99,INK,&lv_font_montserrat_12);auto*l=label(c,number(d.temp,0," °C"),LIST_W-88,7,81,INK);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,d.model,7,28,LIST_W-116,MUTED,&lv_font_montserrat_8);bool ok=!strcmp(d.health,"passed");l=label(c,String(d.zfsStatus[0]?d.zfsStatus:d.health)+"  >",LIST_W-113,28,106,ok||!strcmp(d.zfsStatus,"ONLINE")?MINT:!strcmp(d.health,"failed")?RED:MUTED,&lv_font_montserrat_8);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,"SMART "+String(d.health),7,42,(LIST_W-16),MUTED,&lv_font_montserrat_8);clickable(c,diskGo,(void*)(uintptr_t)i);y+=60;}
}
void diskDetail(){
  int index=-1;for(int i=0;i<snapshot->nDisks;i++)if(!strcmp(snapshot->disks[i].name,selectedDiskName))index=i;
  if(index<0){label(content,"Disk no longer present",12,18,LIST_W,AMBER);return;}
  auto&d=snapshot->disks[index];heading(d.name,d.status,true);label(content,d.model,8,PORTRAIT?43:28,PAGE_W,MUTED,&lv_font_montserrat_10);auto*s=scroll(PORTRAIT?62:46);int i=0;
  cell(s,i++,0,"SMART health",d.health,!strcmp(d.health,"passed")?MINT:!strcmp(d.health,"failed")?RED:MUTED);cell(s,i++,0,"Temperature",number(d.temp,1," °C"),AMBER);cell(s,i++,0,"Read",rate(d.read));cell(s,i++,0,"Write",rate(d.write));
  if(d.zfsStatus[0]){cell(s,i++,0,"ZFS status",d.zfsStatus,!strcmp(d.zfsStatus,"ONLINE")?MINT:AMBER);cell(s,i++,0,"Checksum errors",number(d.zfsChecksumErrors,0));cell(s,i++,0,"ZFS read errors",number(d.zfsReadErrors,0));cell(s,i++,0,"ZFS write errors",number(d.zfsWriteErrors,0));}
  int y=cellBottom(i)+5;section(s,y,"DETAILED SMART");y+=22;i=0;cell(s,i++,y,"Life used",number(d.wear,0,"%"));cell(s,i++,y,"Spare capacity",number(d.spare,0,"%"));cell(s,i++,y,"Media errors",number(d.mediaErrors,0));cell(s,i++,y,"Power-on hours",number(d.powerHours,0,"h"));cell(s,i++,y,"Reallocated sectors",number(d.reallocated,0));cell(s,i++,y,"Pending sectors",number(d.pending,0));note(s,cellBottom(6,y)+5,"Unavailable SMART fields are shown as --.");
}
int sensorFilter=1;const char*const sensorKinds[]={"all","temperature","fan","voltage","power","current"};
void filter(lv_event_t*e){sensorFilter=uintptr_t(lv_event_get_user_data(e));render();}
void sensors(){
  heading("Hardware sensors",String(snapshot->nSensors)+" readings");
  // Six category controls retain 40px targets while matching the compact chips.
  auto*chips=box(content,8,PORTRAIT?40:22,PORTRAIT?PAGE_W-52:252,40,BG,0);lv_obj_add_flag(chips,LV_OBJ_FLAG_SCROLLABLE);lv_obj_set_scroll_dir(chips,LV_DIR_HOR);lv_obj_set_scrollbar_mode(chips,LV_SCROLLBAR_MODE_OFF);const char*names[]={"All","Temp","Fans","Volts","Power","Amps"};
  lv_obj_t*activeChip=nullptr;for(int i=0;i<6;i++){auto*c=box(chips,i*42,0,40,40,sensorFilter==i?(lightTheme?CARD:DARK_SELECTED):CARD,5);if(sensorFilter==i){lv_obj_set_style_border_color(c,color(CYAN),0);activeChip=c;}label(c,names[i],3,14,34,sensorFilter==i?CYAN:MUTED,&lv_font_montserrat_10);clickable(c,filter,(void*)(uintptr_t)i);}
  // Keep the selected category visible when refreshing the narrower strip.
  lv_obj_update_layout(chips);if(activeChip)lv_obj_scroll_to_view(activeChip,LV_ANIM_OFF);auto*power=box(content,VIEW_W-54,PORTRAIT?40:22,46,40,CARD,5);icon(power,4,16,12,14,CYAN);clickable(power,go,(void*)5);
  auto*s=scroll(PORTRAIT?84:66);int y=0,shown=0;String lastChip;
  for(int i=0;i<snapshot->nSensors;i++){auto&v=snapshot->sensors[i];if(sensorFilter&&strcmp(v.kind,sensorKinds[sensorFilter]))continue;shown++;
    if(lastChip!=v.chip){section(s,y,v.chip);lastChip=v.chip;y+=18;}
    uint32_t fg=INK;bool critical=isfinite(v.crit)&&isfinite(v.value)&&v.value>=v.crit;bool hot=isfinite(v.high)&&isfinite(v.value)&&v.value>=v.high;if(critical)fg=RED;else if(hot)fg=AMBER;
    String reading=number(v.value,!strcmp(v.kind,"voltage")||!strcmp(v.kind,"current")?2:!strcmp(v.kind,"fan")?0:1," ")+v.unit;auto*c=box(s,0,y,LIST_W,PORTRAIT?61:43);label(c,v.name,7,6,PORTRAIT?LIST_W-14:168,INK,&lv_font_montserrat_12);auto*l=label(c,reading,PORTRAIT?7:180,PORTRAIT?25:6,PORTRAIT?LIST_W-14:109,fg,fitFont(reading,PORTRAIT?LIST_W-14:109));lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);
    String thresholds=String(v.kind)+" • High "+number(v.high)+" / Crit "+number(v.crit);label(c,thresholds,7,PORTRAIT?44:27,(LIST_W-14),MUTED,fitFont(thresholds,(LIST_W-14),&lv_font_montserrat_10));y+=PORTRAIT?66:48;
  }
  if(!shown){
    const Source*source=nullptr;for(int i=0;i<snapshot->nSources;i++)if(!strcmp(snapshot->sources[i].name,"sensors")){source=&snapshot->sources[i];break;}
    bool healthy=source&&source->enabled&&source->ok;String title,message;
    if(healthy){title=sensorFilter==5?"No current sensors reported":sensorFilter==4?"No power sensors reported":sensorFilter?"No readings in this category":"No sensor readings reported";message=sensorFilter==5?"The reported hardware/driver readings do not expose amperage.":"The sensor source is healthy; this category has no readings.";}
    else{title=source&&!source->enabled?"Sensor source disabled":source?"Hardware sensors unavailable":"Sensor status unavailable";message="Open Health for sensor-source status and collection errors.";}
    auto*c=box(s,0,0,LIST_W,80);label(c,title,7,9,(LIST_W-14),healthy?INK:AMBER,&lv_font_montserrat_12);y=note(c,32,message)+4;lv_obj_set_height(c,y);y+=8;
  }
  if(snapshot->truncated){note(s,y,"Inventory exceeds display limits; see collector for the rest.",AMBER);}
}
void powerDetail(){
  heading("CPU & power","",true);auto*s=scroll(28);bool estimate=!strcmp(snapshot->cpuPowerBasis,"macos-ioreport-estimate");cell(s,0,0,"CPU usage",number(snapshot->cpu,1,"%"),CYAN);cell(s,1,0,estimate?"CPU power ~":"Package power",number(snapshot->watts,1," W"),MINT);cell(s,2,0,"Average frequency",number(snapshot->mhz,0," MHz"));cell(s,3,0,"Busy frequency",number(snapshot->busyMhz,0," MHz"));cell(s,4,0,"CPU temperature",number(snapshot->temp,0," °C"),AMBER);cell(s,5,0,"I/O wait",number(snapshot->iowait,1,"%"));int y=cellBottom(6)+5;if(estimate)y=note(s,y,"CPU package power is an IOReport estimate, not wall-socket power.");smallButton(s,"GPUs • "+String(snapshot->nGpus)+" devices  >",0,y,LIST_W,CYAN,go,(void*)11);y+=50;
  section(s,y,"LOGICAL CPU USAGE / %");y+=22;
  int coreColumns=PORTRAIT?6:8,coreStep=LIST_W/coreColumns;for(int i=0;i<snapshot->nCores;i++){float v=snapshot->cores[i];int heat=isfinite(v)?int(20+constrain(v,0.0f,100.0f)*.65):0;auto*c=box(s,(i%coreColumns)*coreStep,y+(i/coreColumns)*32,coreStep-3,28,CARD,4);lv_obj_set_style_bg_color(c,lv_color_mix(color(CYAN),color(CARD),heat),0);lv_obj_set_style_border_width(c,0,0);auto*index=label(c,String(i),0,2,coreStep-3,INK,&lv_font_montserrat_8);lv_obj_set_style_text_align(index,LV_TEXT_ALIGN_CENTER,0);auto*value=label(c,number(v,0),0,14,coreStep-3,INK,&lv_font_montserrat_10);lv_obj_set_style_text_align(value,LV_TEXT_ALIGN_CENTER,0);}
  if(!snapshot->nCores){row(s,y,"Core readings","Unavailable",AMBER);y+=44;}else y+=((snapshot->nCores+coreColumns-1)/coreColumns)*32+8;
  section(s,y,"IDLE RESIDENCY");y+=22;if(!snapshot->nCstates){row(s,y,"C-state readings","Unavailable",AMBER);y+=44;}else{for(int i=0;i<snapshot->nCstates;i++)cell(s,i,y,snapshot->cstateNames[i],number(snapshot->cstates[i],1,"%"));y+=cellBottom(snapshot->nCstates)+8;}
  smallButton(s,"Memory, load & host I/O",0,y,LIST_W,CYAN,go,(void*)10);
}
void memoryDetail(){
  heading("Memory","",true);auto*s=scroll(28);bool mac=!strcmp(snapshot->memoryBasis,"macos-native");int i=0;cell(s,i++,0,"Used",bytes(snapshot->memUsed),LAVENDER);cell(s,i++,0,"Total",bytes(snapshot->memTotal));if(mac){cell(s,i++,0,"Memory pressure",memoryPressureText(snapshot->memoryPressure),memoryPressureColor(snapshot->memoryPressure));cell(s,i++,0,"Compressed",bytes(snapshot->memCompressed),LAVENDER);cell(s,i++,0,"App memory ~",bytes(snapshot->memApp));cell(s,i++,0,"Wired",bytes(snapshot->memWired));cell(s,i++,0,"Cached files",bytes(snapshot->memCache));cell(s,i++,0,"Available",bytes(snapshot->memAvailable));}cell(s,i++,0,"Swap used",bytes(snapshot->swapUsed));cell(s,i++,0,"Swap total",bytes(snapshot->swapTotal));if(!mac)cell(s,i++,0,"ZFS ARC",bytes(snapshot->arc));cell(s,i++,0,"Load 1 / 5 / 15",number(snapshot->load[0])+" / "+number(snapshot->load[1])+" / "+number(snapshot->load[2]));
  int y=note(s,cellBottom(i)+5,mac?"Native macOS estimate: app + wired + physical compression. Cached files are reclaimable; pressure is reported by macOS.":deviceNode()?"Used = total - available.":"Used = total - available.\nGuest usage can include VM overhead.");section(s,y+5,"HOST / I/O");y+=27;cell(s,0,y,"Host uptime",uptime(snapshot->uptime));cell(s,1,y,"Host address",snapshot->ip);cell(s,2,y,"Network in",rate(snapshot->rx));cell(s,3,y,"Network out",rate(snapshot->tx));cell(s,4,y,"Disk read",rate(snapshot->read));cell(s,5,y,"Disk write",rate(snapshot->write));
}
bool engineLoad(const Gpu&g){return !strcmp(g.utilizationKind,"busiest-engine");}
uint32_t gpuStateColor(const Gpu&g){return !strcmp(g.status,"active")?MINT:!strcmp(g.status,"inventory")?MUTED:AMBER;}
String gpuOwner(const Gpu&g){if(!strcmp(g.owner,"host"))return "Host";if(!strncmp(g.owner,"vm:",3)){int id=atoi(g.owner+3);for(int i=0;i<snapshot->nGuests;i++)if(snapshot->guests[i].id==id)return String(g.owner)+" • "+snapshot->guests[i].name;}return g.owner[0]?String(g.owner):String("Unknown owner");}
String gpuMemory(const Gpu&g){return isfinite(g.memTotal)?bytes(g.memUsed)+" / "+bytes(g.memTotal):bytes(g.memUsed);}
void gpuList(){
  heading("GPUs",String(snapshot->nGpus)+" devices",true);auto*s=scroll(28);gpuIdentities();
  if(!snapshot->nGpus){row(s,0,"GPU inventory","Unavailable",AMBER);note(s,48,"The collector has not supplied GPU inventory.");return;}
  int step=PORTRAIT?97:79;for(int i=0;i<snapshot->nGpus;i++){auto&g=snapshot->gpus[i];auto*c=box(s,0,i*step,LIST_W,step-5);icon(c,5,7,7,14,CYAN);label(c,g.name,27,7,PORTRAIT?LIST_W-34:193,INK,&lv_font_montserrat_12);auto*state=label(c,g.status,LIST_W-77,PORTRAIT?27:9,70,gpuStateColor(g),&lv_font_montserrat_8);lv_obj_set_style_text_align(state,LV_TEXT_ALIGN_RIGHT,0);label(c,gpuOwner(g)+" • "+g.kind,7,PORTRAIT?44:28,LIST_W-14,MUTED,&lv_font_montserrat_10);
    String metrics=String(engineLoad(g)?"Engine ":"GPU ")+number(g.utilization,0,"%")+" • "+number(g.temp,0," °C")+" • "+gpuMemory(g);label(c,metrics,7,PORTRAIT?61:45,LIST_W-14,INK,fitFont(metrics,LIST_W-14,&lv_font_montserrat_10));double age=sampleAge(g.age,g.updated);label(c,String(!strcmp(g.status,"inventory")?"Inventory only • ":g.error[0]?"Limited telemetry • ":"Updated • ")+number(age,0,"s ago")+"  >",7,PORTRAIT?77:60,LIST_W-14,g.error[0]?AMBER:MUTED,&lv_font_montserrat_8);clickable(c,gpuGo,(void*)(uintptr_t)i);
  }
  if(snapshot->truncated)note(s,snapshot->nGpus*step,"Inventory exceeds display limits; additional devices may be omitted.",AMBER);
}
void gpuDetail(){
  int index=-1;for(int i=0;i<snapshot->nGpus;i++)if(!strcmp(snapshot->gpus[i].id,selectedGpuId))index=i;
  if(index<0){heading("GPU unavailable","",true);note(scroll(28),0,"This GPU is no longer in the current feed.",AMBER);return;}
  auto&g=snapshot->gpus[index];heading(g.name,g.status,true);label(content,gpuOwner(g),8,PORTRAIT?43:29,PAGE_W,MUTED,&lv_font_montserrat_10);auto*s=scroll(PORTRAIT?62:46);cell(s,0,0,engineLoad(g)?"Engine load":"GPU utilization",number(g.utilization,1,"%"),CYAN);cell(s,1,0,!strcmp(g.kind,"integrated")?"Shared RAM":!strcmp(g.kind,"discrete")?"VRAM":"GPU RAM",gpuMemory(g),LAVENDER);cell(s,2,0,"Temperature",number(g.temp,0," °C"),AMBER);cell(s,3,0,"Power",number(g.power,1," W"),MINT);cell(s,4,0,"Graphics clock",number(g.graphicsMhz,0," MHz"));cell(s,5,0,"Memory clock",number(g.memoryMhz,0," MHz"));cell(s,6,0,"Fan",number(g.fan,0,"%"));cell(s,7,0,"Sample age",number(sampleAge(g.age,g.updated),0,"s"));int y=note(s,cellBottom(8)+5,g.name,INK);
  if(g.error[0])y=note(s,y,g.error,AMBER);if(!strcmp(g.status,"inventory"))y=note(s,y,"Inventory is known; live GPU telemetry is unavailable.",AMBER);if(!strcmp(g.kind,"integrated"))y=note(s,y,"Integrated GPU uses shared system RAM. Dedicated GPU memory is not reported.");if(engineLoad(g))y=note(s,y,"Engine load is the busiest DRM engine, not total GPU utilization.");
  y=note(s,y,String("Vendor: ")+(g.vendor[0]?g.vendor:"--")+" • "+g.kind);y=note(s,y,"Driver: "+String(g.driver[0]?g.driver:"--"));note(s,y,"Device ID: "+String(g.id));
}
void subnodesList(){
  uint8_t count=subnodes(snapshot->node.id),healthy=subnodes(snapshot->node.id,true);heading("Subnodes",String(healthy)+" healthy");auto*s=scroll(28);fillNodeLinks();int y=0;
  for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(strcmp(n.parentId,snapshot->node.id))continue;bool fresh=nodeSummaryFresh(n);const char*state=nodeSummaryState(n);auto*c=box(s,0,y,LIST_W,63,CARD,7);auto*tag=box(c,7,7,25,14,lightTheme?0xe8eef4:0x26384c,3);lv_obj_set_style_border_width(tag,0,0);label(tag,!strcmp(n.platform,"router")?"NET":"DEV",3,3,22,MUTED,&lv_font_montserrat_8);label(c,n.name,38,7,LIST_W-113,INK,&lv_font_montserrat_12);auto*status=label(c,state,LIST_W-72,9,65,fresh&&!strcmp(state,"Healthy")?MINT:AMBER,&lv_font_montserrat_10);lv_obj_set_style_text_align(status,LV_TEXT_ALIGN_RIGHT,0);
    label(c,String("CPU ")+number(fresh?n.summary.cpu:NAN,1,"%"),7,28,PORTRAIT?102:128,MUTED,&lv_font_montserrat_10);auto*ram=label(c,String("RAM ")+number(fresh?n.summary.memory:NAN,0,"%  >"),PORTRAIT?111:135,28,PORTRAIT?LIST_W-118:154,LAVENDER,&lv_font_montserrat_10);lv_obj_set_style_text_align(ram,LV_TEXT_ALIGN_RIGHT,0);label(c,String(nodeTypeName(n))+" • "+number(fresh?n.summary.temp:NAN,0," °C")+" • "+number(sampleAge(n.summary.age,n.summary.generated),0,"s ago"),7,43,LIST_W-14,fresh?MUTED:AMBER,&lv_font_montserrat_8);progress(c,7,56,LIST_W-14,fresh?n.summary.memory:NAN,LAVENDER);clickable(c,nodeGo,(void*)(uintptr_t)i);y+=68;
  }
  if(!count)note(s,0,"No subnodes configured for this node.");
}
int subnodeCards(lv_obj_t*s,int y){
  fillNodeLinks();int parent=nodeIndexForId(*snapshot,snapshot->node.parentId);if(parent>=0){smallButton(s,String("Back to ")+snapshot->nodes[parent].name,0,y,LIST_W,CYAN,nodeGo,(void*)(uintptr_t)parent);y+=46;}
  if(!subnodes(snapshot->node.id))return y;section(s,y,"SUBNODES");y+=20;
  for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(strcmp(n.parentId,snapshot->node.id))continue;bool fresh=nodeSummaryFresh(n);const char*state=nodeSummaryState(n);auto*c=box(s,0,y,LIST_W,72,CARD,7);label(c,n.name,9,8,LIST_W-92,INK,&lv_font_montserrat_12);auto*status=label(c,state,LIST_W-82,10,72,fresh&&!strcmp(state,"Healthy")?MINT:AMBER,&lv_font_montserrat_8);lv_obj_set_style_text_align(status,LV_TEXT_ALIGN_RIGHT,0);String primary=summaryPrimary(n);label(c,primary,9,31,LIST_W-18,fresh?CYAN:MUTED,&lv_font_montserrat_10);label(c,String(nodeTypeName(n))+" • "+number(fresh?n.summary.temp:NAN,0," °C")+"  >",9,53,LIST_W-18,MUTED,&lv_font_montserrat_8);clickable(c,nodeGo,(void*)(uintptr_t)i);y+=78;}
  return y+4;
}
void alerts(){
  heading("Health & sources",String(snapshot->nAlerts)+" alerts",!deviceNode());auto*s=scroll(28);int y=0;
  if(stale()){row(s,y,"Snapshot age",number(ageSeconds(),0,"s (stale)"),AMBER);y+=44;}
  for(int i=0;i<snapshot->nAlerts;i++){auto&a=snapshot->alerts[i];lv_point_t textSize;lv_text_get_size(&textSize,a.message,&lv_font_montserrat_12,0,0,(LIST_W-20),LV_TEXT_FLAG_NONE);int h=35+textSize.y;auto*c=box(s,0,y,LIST_W,h);lv_obj_set_style_border_color(c,color(!strcmp(a.severity,"critical")?RED:AMBER),0);lv_obj_set_style_border_width(c,2,0);lv_obj_set_style_border_side(c,LV_BORDER_SIDE_LEFT,0);label(c,a.severity,10,7,(LIST_W-18),!strcmp(a.severity,"critical")?RED:AMBER,&lv_font_montserrat_10);auto*l=label(c,a.message,10,26,(LIST_W-20),INK);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=h+6;}
  if(!snapshot->nAlerts&&!stale()){row(s,y,"Active alerts","None",MINT);y+=44;}
  if(!deviceNode()||snapshot->nFaults||isfinite(snapshot->faultEvents24h)){section(s,y,deviceNode()?"DEVICE FAULTS / LAST 24 HOURS":"PROXMOX FAULTS / LAST 24 HOURS");y+=18;
  row(s,y,"Segfaults",number(snapshot->segfaults24h,0),isfinite(snapshot->segfaults24h)?snapshot->segfaults24h>0?AMBER:MINT:MUTED);y+=44;row(s,y,"Fault events",number(snapshot->faultEvents24h,0),isfinite(snapshot->faultEvents24h)?snapshot->faultEvents24h>0?AMBER:MINT:MUTED);y+=44;row(s,y,"Last recorded fault",since(snapshot->lastFaultAt));y+=44;
  section(s,y,"RECENT FAULT HISTORY / "+String(snapshot->faultLookbackDays)+" DAYS");y+=18;
  if(!snapshot->nFaults){row(s,y,"Recent fault history",isfinite(snapshot->faultEvents24h)?"None":"Unavailable",isfinite(snapshot->faultEvents24h)?MINT:AMBER);y+=44;}
  for(int i=0;i<snapshot->nFaults;i++){auto&f=snapshot->faults[i];lv_point_t textSize;lv_text_get_size(&textSize,f.message,&lv_font_montserrat_12,0,0,(LIST_W-20),LV_TEXT_FLAG_NONE);int h=36+textSize.y;auto*c=box(s,0,y,LIST_W,h);label(c,f.kind,10,8,LIST_W-132,!strcmp(f.kind,"segfault")?AMBER:RED,&lv_font_montserrat_10);auto*l=label(c,since(f.timestamp),LIST_W-116,8,106,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);l=label(c,f.message,10,28,(LIST_W-20),INK);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=h+6;}
  }
  section(s,y,"DATA SOURCES");y+=18;for(int i=0;i<snapshot->nSources;i++){auto&v=snapshot->sources[i];String message=v.error[0]?String(v.error):!v.enabled?"Source disabled by configuration":"Source reporting normally";lv_point_t textSize;displayTextSize(&textSize,message,&lv_font_montserrat_10,0,0,(LIST_W-20),LV_TEXT_FLAG_NONE);int h=(PORTRAIT?60:42)+textSize.y;auto*c=box(s,0,y,LIST_W,h);label(c,v.name,10,7,PORTRAIT?LIST_W-20:152,INK);double age=isfinite(v.age)?v.age+(millis()-snapshot->received)/1000.0:NAN;auto*l=label(c,!v.enabled?"DISABLED":v.ok?"OK  "+number(age,0,"s")+" ago":"UNAVAILABLE",PORTRAIT?10:164,PORTRAIT?25:7,PORTRAIT?LIST_W-20:121,!v.enabled?MUTED:v.ok?MINT:AMBER,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);auto*errorLabel=label(c,message,10,PORTRAIT?50:32,(LIST_W-20),MUTED,&lv_font_montserrat_10);lv_label_set_long_mode(errorLabel,LV_LABEL_LONG_WRAP);lv_obj_set_height(errorLabel,LV_SIZE_CONTENT);y+=h+6;}
  if(!snapshot->nSources){row(s,y,"Source health","Unavailable",AMBER);y+=44;}
  if(deviceNode()&&snapshot->nInterfaces){section(s,y,"NETWORK INTERFACES");y+=20;for(int i=0;i<snapshot->nInterfaces;i++){auto&n=snapshot->interfaces[i];row(s,y,n.name,String(n.status)+" • "+number(n.speed,0," Mb/s"),!strcmp(n.status,"up")?MINT:!strcmp(n.status,"down")?AMBER:MUTED);y+=44;y=note(s,y,"RX "+rate(n.rx)+" • TX "+rate(n.tx));if(isfinite(n.errorsIn)||isfinite(n.errorsOut))y=note(s,y,"Errors in/out: "+number(n.errorsIn,0)+" / "+number(n.errorsOut,0));if(isfinite(n.dropsIn)||isfinite(n.dropsOut))y=note(s,y,"Drops in/out: "+number(n.dropsIn,0)+" / "+number(n.dropsOut,0));}}
  if(deviceNode()){row(s,y,"Platform",platformName(snapshot->node.platform));y+=44;y=note(s,y,"Device: "+String(snapshot->node.name));if(snapshot->node.address[0])y=note(s,y,"Address: "+String(snapshot->node.address));if(snapshot->osRelease[0])y=note(s,y,"OS release: "+String(snapshot->osRelease));if(snapshot->osVersion[0])y=note(s,y,"OS version: "+String(snapshot->osVersion));if(snapshot->architecture[0]){row(s,y,"Architecture",snapshot->architecture);y+=44;}if(snapshot->uptimeScope[0])y=note(s,y,"Uptime scope: "+String(snapshot->uptimeScope));if(isfinite(snapshot->batteryPercent)){row(s,y,"Battery",number(snapshot->batteryPercent,0,"%"));y+=44;if(snapshot->batteryPlugged>=0){row(s,y,"Power source",snapshot->batteryPlugged?"Plugged in":"Battery");y+=44;}if(isfinite(snapshot->batteryRemaining)&&snapshot->batteryRemaining>=0){row(s,y,"Battery remaining",uptime(snapshot->batteryRemaining));y+=44;}}}
  y=subnodeCards(s,y);
  if(snapshot->truncated)note(s,y,"Some entries exceed the display inventory limits.",AMBER);
}
void persistDisplay(){Preferences p;if(p.begin("display",false)){p.putUChar("brightness",brightness);p.putBool("rotate",autoRotate);bool savedVolume=p.putUChar("volume",soundVolume)>0&&p.getUChar("volume",255)==soundVolume;bool savedSound=p.putBool("sound",soundEnabled)>0&&p.getBool("sound",!soundEnabled)==soundEnabled;bool savedLight=p.putBool("light",lightTheme)>0&&p.getBool("light",!lightTheme)==lightTheme;bool savedMode=p.putUChar("themeMode",themeMode)>0&&p.getUChar("themeMode",255)==themeMode;bool savedDim=p.putBool("dim",idleDim)>0&&p.getBool("dim",!idleDim)==idleDim;bool savedZone=p.putUChar("clockZone",clockZone)>0&&p.getUChar("clockZone",255)==clockZone;if(!savedVolume||!savedSound||!savedLight||!savedMode||!savedDim||!savedZone)Serial.println("Display preference did not verify");p.end();}}
void brightnessChanged(lv_event_t*e){auto *input=lv_event_get_indev(e);if(input&&lv_indev_get_scroll_obj(input))return;brightness=lv_slider_get_value((lv_obj_t*)lv_event_get_target(e));boardBacklight(brightness);persistDisplay();}
void volumeChanged(lv_event_t*e){
  auto*slider=(lv_obj_t*)lv_event_get_target(e);auto*value=(lv_obj_t*)lv_event_get_user_data(e);
  auto*input=lv_event_get_indev(e);if(input&&lv_indev_get_scroll_obj(input)){lv_slider_set_value(slider,soundVolume,LV_ANIM_OFF);lv_label_set_text(value,(String(soundVolume)+"%").c_str());return;}
  uint8_t selected=clampSoundVolume(lv_slider_get_value(slider));lv_label_set_text(value,(String(selected)+"%").c_str());
  if(lv_event_get_code(e)==LV_EVENT_RELEASED){soundVolume=selected;boardAudioVolume(soundVolume);persistDisplay();}
}
void rotateChanged(lv_event_t*e){auto*sw=(lv_obj_t*)lv_event_get_user_data(e);if(!sw)sw=(lv_obj_t*)lv_event_get_target(e);autoRotate=!autoRotate;if(autoRotate)lv_obj_add_state(sw,LV_STATE_CHECKED);else lv_obj_remove_state(sw,LV_STATE_CHECKED);lastRotate=millis();persistDisplay();}
void dimChanged(lv_event_t*e){auto*sw=(lv_obj_t*)lv_event_get_user_data(e);if(!sw)sw=(lv_obj_t*)lv_event_get_target(e);idleDim=!idleDim;if(idleDim)lv_obj_add_state(sw,LV_STATE_CHECKED);else{lv_obj_remove_state(sw,LV_STATE_CHECKED);if(dimmed){dimmed=false;boardBacklight(brightness);}}lastTouch=millis();persistDisplay();}
void themeChanged(lv_event_t*e){uint8_t mode=uintptr_t(lv_event_get_user_data(e));if(mode>THEME_AUTO||mode==themeMode)return;themeMode=mode;applyAppearance();persistDisplay();render();}
void clockZoneChanged(lv_event_t*){clockZone=(clockZone+1)%DISPLAY_CLOCK_ZONE_COUNT;applyAppearance();persistDisplay();render();}
void soundChanged(lv_event_t*){soundEnabled=!soundEnabled;boardAudioEnable(soundEnabled);persistDisplay();render();}
void soundTest(lv_event_t*){boardAudioPlay(AudioCue::Test);showNotification("Sound test","Short notification chime • tap to dismiss");}
void networkSetup(lv_event_t*){manualDemo=false;requestSetup=true;page=8;render();}
void exitDemo(lv_event_t*){manualDemo=false;page=ALL_NODES_PAGE;render();}
void startDemo(lv_event_t*){manualDemo=true;makeDemo(*snapshot);page=ALL_NODES_PAGE;historyCount=0;for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}render();}
void readManagement(){
  readConnectionSettings(connectionUi);readNodeConfiguration(nodeConfigUi);if(wifiSavePending&&!connectionUi.busy){if(connectionUi.saved)wifiDraft[1]=wifiDraft[3]=wifiDraft[4]="";wifiSavePending=false;}
  if((nodeSavePending||nodeDeletePending)&&!nodeConfigUi.busy){if(nodeConfigUi.saved){nodeDraft[2]="";nodeSavePending=nodeDeletePending=false;page=15;}else if(nodeConfigUi.message[0])nodeSavePending=nodeDeletePending=false;}
}
void openConnection(lv_event_t*){manualDemo=false;readSnapshot(*snapshot,net);readManagement();wifiDraft[0]=connectionUi.ssid;wifiDraft[1]="";wifiDraft[2]=connectionUi.endpoint;wifiDraft[3]=wifiDraft[4]="";wifiClearPassword=false;formMessage="";openPage(14);}
void openNodeSettings(lv_event_t*){manualDemo=false;readSnapshot(*snapshot,net);formMessage="";requestNodeConfiguration();openPage(15);}
void refreshNodes(lv_event_t*){requestNodeConfiguration();render();}
String&fieldDraft(){return fieldOrigin==FLIPPER_PAGE?flipperPinDraft:fieldOrigin==14?wifiDraft[fieldIndex]:nodeDraft[fieldIndex];}
bool secretField(){return fieldOrigin==FLIPPER_PAGE?true:fieldOrigin==14?(fieldIndex==1||fieldIndex==3||fieldIndex==4):fieldIndex==2;}
void fieldChanged(lv_event_t*e){fieldDraft()=lv_textarea_get_text(lv_event_get_current_target_obj(e));if(fieldDraft().length()){if(fieldOrigin==14&&fieldIndex==1)wifiClearPassword=false;else if(fieldOrigin==16&&fieldIndex==2)nodeClearSecret=false;}}
void finishField(bool cancel){if(fieldOrigin==FLIPPER_PAGE){
  bool submitted=false;if(!cancel&&flipperPinDraft.length()==6){uint32_t code=0;bool digits=true;for(size_t i=0;i<6;i++){char c=flipperPinDraft[i];if(c<'0'||c>'9'){digits=false;break;}code=code*10+uint32_t(c-'0');}if(digits)submitted=flipper::submitPasskey(code);}
  flipperPinDraft="";fieldOriginal="";if(!submitted)flipper::confirmPairing(false);page=FLIPPER_PAGE;render();return;
}if(cancel){fieldDraft()=fieldOriginal;if(fieldOrigin==14&&fieldIndex==1)wifiClearPassword=fieldOriginalToggle;else if(fieldOrigin==16&&fieldIndex==2)nodeClearSecret=fieldOriginalToggle;}page=fieldOrigin;render();}
void fieldDone(lv_event_t*e){finishField(lv_event_get_code(e)==LV_EVENT_CANCEL);}
void clearField(lv_event_t*){lv_textarea_set_text(fieldArea,"");}
void editField(lv_event_t*e){fieldOrigin=page;fieldIndex=uintptr_t(lv_event_get_user_data(e));fieldOriginal=fieldDraft();fieldOriginalToggle=fieldOrigin==14?wifiClearPassword:nodeClearSecret;page=18;render();}
void keyboardInput(lv_event_t*e){auto*o=lv_event_get_current_target_obj(e);auto*input=lv_event_get_indev(e);lv_point_t point=tapOrigin;if(input)lv_indev_get_point(input,&point);int dx=point.x-tapOrigin.x,dy=point.y-tapOrigin.y;if(tapTarget!=o||tapDragged||consumeWakeTouch||dx*dx+dy*dy>=TAP_SLOP_PX*TAP_SLOP_PX)return;lv_keyboard_def_event_cb(e);if(keyboard)lv_buttonmatrix_set_button_ctrl_all(keyboard,lv_buttonmatrix_ctrl_t(LV_BUTTONMATRIX_CTRL_CLICK_TRIG|LV_BUTTONMATRIX_CTRL_NO_REPEAT));}
void fieldEditor(){
  heading("","",true);String fieldTitle=fieldOrigin==FLIPPER_PAGE?String("Flipper pairing PIN"):fieldOrigin==14?wifiFields[fieldIndex]:nodeFields[fieldIndex];label(content,fieldTitle,35,7,VIEW_W-103,INK,fitFont(fieldTitle,VIEW_W-103));auto*b=box(content,VIEW_W-60,0,60,40,BG,0);label(b,"Clear",9,12,48,CYAN,&lv_font_montserrat_10);clickable(b,clearField);
  fieldArea=lv_textarea_create(content);lv_obj_remove_flag(fieldArea,LV_OBJ_FLAG_SCROLL_ON_FOCUS);lv_obj_set_pos(fieldArea,8,PORTRAIT?46:38);lv_obj_set_size(fieldArea,PAGE_W,PORTRAIT?54:40);lv_textarea_set_one_line(fieldArea,true);const uint16_t wifiLimits[]={32,64,240,192,192},nodeLimits[]={63,240,256,32,8,8,8};lv_textarea_set_max_length(fieldArea,fieldOrigin==FLIPPER_PAGE?6:fieldOrigin==14?wifiLimits[fieldIndex]:nodeLimits[fieldIndex]);lv_textarea_set_password_mode(fieldArea,secretField());lv_textarea_set_password_show_time(fieldArea,0);if(fieldOrigin==FLIPPER_PAGE)lv_textarea_set_accepted_chars(fieldArea,"0123456789");else if(fieldOrigin==16&&fieldIndex>=4)lv_textarea_set_accepted_chars(fieldArea,"0123456789.");lv_textarea_set_text(fieldArea,fieldDraft().c_str());lv_textarea_set_cursor_pos(fieldArea,LV_TEXTAREA_CURSOR_LAST);lv_obj_set_style_bg_color(fieldArea,color(CARD),0);lv_obj_set_style_text_color(fieldArea,color(INK),0);lv_obj_set_style_text_font(fieldArea,&lv_font_montserrat_14,0);lv_obj_set_style_border_color(fieldArea,color(CYAN),0);lv_obj_set_style_border_width(fieldArea,1,0);lv_obj_set_style_radius(fieldArea,7,0);lv_obj_set_style_pad_all(fieldArea,8,0);lv_obj_add_event_cb(fieldArea,fieldChanged,LV_EVENT_VALUE_CHANGED,nullptr);
  keyboard=lv_keyboard_create(lv_screen_active());lv_obj_set_align(keyboard,LV_ALIGN_TOP_LEFT);int keyboardY=PORTRAIT?HEADER_H+108:118;lv_obj_set_pos(keyboard,0,keyboardY);lv_obj_set_size(keyboard,VIEW_W,VIEW_H-keyboardY);lv_keyboard_set_textarea(keyboard,fieldArea);if(fieldOrigin==FLIPPER_PAGE||(fieldOrigin==16&&fieldIndex>=4))lv_keyboard_set_mode(keyboard,LV_KEYBOARD_MODE_NUMBER);lv_keyboard_set_popovers(keyboard,false);lv_obj_remove_flag(keyboard,LV_OBJ_FLAG_SCROLLABLE);lv_obj_add_flag(keyboard,LV_OBJ_FLAG_PRESS_LOCK);lv_obj_set_style_bg_color(keyboard,color(BG),LV_PART_MAIN);lv_obj_set_style_border_width(keyboard,0,LV_PART_MAIN);lv_obj_set_style_pad_all(keyboard,3,LV_PART_MAIN);lv_obj_set_style_pad_row(keyboard,3,LV_PART_MAIN);lv_obj_set_style_pad_column(keyboard,2,LV_PART_MAIN);lv_obj_set_style_bg_color(keyboard,color(CARD),LV_PART_ITEMS);lv_obj_set_style_bg_color(keyboard,color(EDGE),LV_PART_ITEMS|LV_STATE_CHECKED);lv_obj_set_style_text_color(keyboard,color(INK),LV_PART_ITEMS|LV_STATE_CHECKED);lv_obj_set_style_bg_color(keyboard,color(CYAN),LV_PART_ITEMS|LV_STATE_PRESSED);lv_obj_set_style_text_color(keyboard,color(CARD),LV_PART_ITEMS|LV_STATE_PRESSED);lv_obj_set_style_bg_opa(keyboard,LV_OPA_COVER,LV_PART_ITEMS);lv_obj_set_style_text_color(keyboard,color(INK),LV_PART_ITEMS);lv_obj_set_style_text_font(keyboard,&lv_font_montserrat_12,LV_PART_ITEMS);lv_obj_set_style_border_width(keyboard,1,LV_PART_ITEMS);lv_obj_set_style_border_color(keyboard,color(EDGE),LV_PART_ITEMS);lv_obj_set_style_radius(keyboard,4,LV_PART_ITEMS);lv_buttonmatrix_set_button_ctrl_all(keyboard,lv_buttonmatrix_ctrl_t(LV_BUTTONMATRIX_CTRL_CLICK_TRIG|LV_BUTTONMATRIX_CTRL_NO_REPEAT));lv_obj_remove_event_cb(keyboard,lv_keyboard_def_event_cb);lv_obj_add_event_cb(keyboard,tapGuard,LV_EVENT_ALL,nullptr);lv_obj_add_event_cb(keyboard,keyboardInput,LV_EVENT_VALUE_CHANGED,nullptr);lv_obj_add_event_cb(keyboard,fieldDone,LV_EVENT_READY,nullptr);lv_obj_add_event_cb(keyboard,fieldDone,LV_EVENT_CANCEL,nullptr);
}
void formField(lv_obj_t*p,int y,const char*key,const String&value,uint8_t index){auto*c=box(p,0,y,LIST_W,48,CARD,7);label(c,key,9,7,(LIST_W-25),MUTED,&lv_font_montserrat_10);label(c,value,9,24,(LIST_W-31),INK);label(c,">",LIST_W-17,23,10,CYAN,&lv_font_montserrat_10);clickable(c,editField,(void*)(uintptr_t)index);}
void toggleWifiClear(lv_event_t*){wifiClearPassword=!wifiClearPassword;if(wifiClearPassword)wifiDraft[1]="";render();}
void saveConnection(lv_event_t*){
  if(wifiDraft[0].length()>32||wifiDraft[1].length()>64||wifiDraft[2].length()>240||wifiDraft[3].length()>192||wifiDraft[4].length()>192){formMessage="A field exceeds its byte limit.";render();return;}ConnectionUpdate update;strlcpy(update.ssid,wifiDraft[0].c_str(),sizeof(update.ssid));strlcpy(update.password,wifiDraft[1].c_str(),sizeof(update.password));strlcpy(update.endpoint,wifiDraft[2].c_str(),sizeof(update.endpoint));strlcpy(update.displayToken,wifiDraft[3].c_str(),sizeof(update.displayToken));strlcpy(update.setupToken,wifiDraft[4].c_str(),sizeof(update.setupToken));update.clearPassword=wifiClearPassword;if(requestConnectionUpdate(update)){wifiSavePending=true;formMessage="";}else formMessage="A settings save is already in progress.";render();
}
void connectionForm(){
  heading("Wi-Fi & collector","",true);auto*s=scroll(28);int y=0;if(formMessage.length())y=note(s,y,formMessage,RED);if(connectionUi.message[0])y=note(s,y,connectionUi.message,connectionUi.busy?CYAN:AMBER);formField(s,y,wifiFields[0],wifiDraft[0].length()?wifiDraft[0]:String("Enter Wi-Fi name"),0);y+=54;formField(s,y,wifiFields[1],wifiDraft[1].length()?String("•••• (new password)"):connectionUi.passwordSaved&&wifiDraft[0]==String(connectionUi.ssid)?String("Saved • blank keeps current"):String("Blank = open network"),1);y+=54;formField(s,y,wifiFields[2],wifiDraft[2].length()?wifiDraft[2]:String("Enter snapshot URL"),2);y+=54;formField(s,y,wifiFields[3],wifiDraft[3].length()?String("•••• (new token)"):connectionUi.displayTokenSaved?String("Saved • blank keeps current"):String("Enter read-only display token"),3);y+=54;formField(s,y,wifiFields[4],wifiDraft[4].length()?String("•••• (new setup token)"):connectionUi.setupTokenSaved?String("Paired • blank keeps current"):String("Optional: pair node management"),4);y+=54;
  auto*c=box(s,0,y,LIST_W,40);label(c,wifiClearPassword?"Clear saved Wi-Fi password: ON":"Clear saved Wi-Fi password: OFF",9,13,(LIST_W-18),wifiClearPassword?AMBER:MUTED,&lv_font_montserrat_10);clickable(c,toggleWifiClear);y+=46;y=note(s,y,"Blank Wi-Fi password keeps it for the same network. Tokens stay saved only for the same collector URL; pair setup access separately.");smallButton(s,connectionUi.busy?"Saving...":"Save & connect",0,y,LIST_W,CYAN,saveConnection);y+=48;smallButton(s,"Advanced browser setup / HTTPS CA",0,y,LIST_W,MUTED,networkSetup);y+=48;note(s,y,"Passwords and tokens are masked and never read back.");
}
void beginNode(const ConfigNode&node,bool existing){editingNode=node;nodeExisting=existing;nodeClearSecret=false;nodeDraft[0]=node.name;nodeDraft[1]=node.url;nodeDraft[2]="";nodeDraft[3]=existing?String(node.id):String("");nodeDraft[4]=number(node.poll,1);nodeDraft[5]=number(node.timeout,1);nodeDraft[6]=number(node.ttl,1);formMessage="";openPage(16);}
void editNode(lv_event_t*e){size_t i=uintptr_t(lv_event_get_user_data(e));if(i<nodeConfigUi.count)beginNode(nodeConfigUi.nodes[i],true);}
void addNode(lv_event_t*){if(nodeConfigUi.count>=MAX_CONFIG_NODES||activeConfiguredNodes(nodeConfigUi)>=MAX_NODES||activeConfiguredRoots(nodeConfigUi)>=4)return;ConfigNode node;strlcpy(node.type,"server-feed",sizeof(node.type));node.poll=5;node.timeout=3;node.ttl=15;beginNode(node,false);}
void setNodeType(lv_event_t*e){if(nodeExisting)return;int selected=uintptr_t(lv_event_get_user_data(e));if(selected==0&&nodeConfigUi.localEnabled){formMessage="The local node is already configured.";render();return;}strlcpy(editingNode.type,selected==0?(!strcmp(nodeConfigUi.localType,"server")?"local-server":"local-proxmox"):selected==1?"klipper":selected==2?"proxmox-feed":"server-feed",sizeof(editingNode.type));editingNode.platform[0]=0;if(selected==0)strlcpy(editingNode.platform,nodeConfigUi.localPlatform,sizeof(editingNode.platform));if(selected==0){strlcpy(editingNode.id,nodeConfigUi.localId,sizeof(editingNode.id));nodeDraft[0]=nodeConfigUi.localName;nodeDraft[3]=nodeConfigUi.localId;}else{editingNode.id[0]=0;nodeDraft[3]="";}formMessage="";render();}
void setLocalNodeMode(lv_event_t*e){if(!localNodeType(editingNode.type))return;bool server=uintptr_t(lv_event_get_user_data(e));if(!server&&nodeConfigUi.localPlatform[0]&&strcmp(nodeConfigUi.localPlatform,"linux")){formMessage="Proxmox collection requires a Linux host.";render();return;}strlcpy(editingNode.type,server?"local-server":"local-proxmox",sizeof(editingNode.type));strlcpy(editingNode.platform,server?nodeConfigUi.localPlatform:"linux",sizeof(editingNode.platform));formMessage="";render();}
void cycleNodePlatform(lv_event_t*){if(localNodeType(editingNode.type))return;const char*values[]={"","linux","macos","windows","router","other"};int next=0;for(int i=0;i<6;i++)if(!strcmp(editingNode.platform,values[i]))next=(i+1)%6;strlcpy(editingNode.platform,values[next],sizeof(editingNode.platform));render();}
void toggleNodeEnabled(lv_event_t*){editingNode.enabled=!editingNode.enabled;render();}
void toggleNodeSecret(lv_event_t*){nodeClearSecret=!nodeClearSecret;if(nodeClearSecret)nodeDraft[2]="";render();}
double draftNumber(const String&s){char*end;double result=strtod(s.c_str(),&end);return end!=s.c_str()&&!*end&&isfinite(result)?result:NAN;}
void saveNode(lv_event_t*){
  if(!nodeConfigUi.available){formMessage="Load collector settings before saving.";render();return;}if(nodeDraft[0].length()>63||nodeDraft[1].length()>240||nodeDraft[2].length()>256||(!nodeExisting&&!localNodeType(editingNode.type)&&nodeDraft[3].length()>32)){formMessage="A node field exceeds its byte limit.";render();return;}NodeConfigDraft draft;draft.node=editingNode;if(localNodeType(draft.node.type))strlcpy(draft.node.platform,deviceNodeType(draft.node.type)?nodeConfigUi.localPlatform:"linux",sizeof(draft.node.platform));strlcpy(draft.node.name,nodeDraft[0].c_str(),sizeof(draft.node.name));strlcpy(draft.node.url,nodeDraft[1].c_str(),sizeof(draft.node.url));if(!nodeExisting&&!localNodeType(draft.node.type))strlcpy(draft.node.id,nodeDraft[3].c_str(),sizeof(draft.node.id));strlcpy(draft.secret,nodeDraft[2].c_str(),sizeof(draft.secret));draft.clearSecret=nodeClearSecret;draft.node.poll=draftNumber(nodeDraft[4]);draft.node.timeout=draftNumber(nodeDraft[5]);draft.node.ttl=draftNumber(nodeDraft[6]);if(!localNodeType(draft.node.type)&&(!isfinite(draft.node.poll)||(!agentNodeType(draft.node.type)&&!isfinite(draft.node.timeout))||!isfinite(draft.node.ttl))){formMessage="Enter valid polling, timeout and TTL numbers.";render();return;}if(requestNodeUpsert(draft,nodeConfigUi.version)){nodeSavePending=true;formMessage="";}else formMessage="Check the node ID, or wait for the active settings request.";render();
}
void confirmDelete(lv_event_t*){strlcpy(deleteNodeId,editingNode.id,sizeof(deleteNodeId));openPage(17);}
void deleteConfirmed(lv_event_t*){if(requestNodeDelete(deleteNodeId,nodeConfigUi.version)){nodeDeletePending=true;formMessage="";}else formMessage="Wait for the active settings request, then retry.";render();}
void cancelDelete(lv_event_t*){openPage(16);}
void nodeManager(){
  heading("Manage nodes",String(activeConfiguredNodes(nodeConfigUi))+" active / "+String(nodeConfigUi.count)+" saved",true);auto*s=scroll(28);int y=0;if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,nodeConfigUi.busy?CYAN:AMBER);if(!nodeConfigUi.available){smallButton(s,"Load / refresh settings",0,y,LIST_W,CYAN,refreshNodes);y+=48;smallButton(s,"Pair setup token / Wi-Fi",0,y,LIST_W,MUTED,openConnection);return;}y=note(s,y,"Pair new device agents in the collector web console. Polling feeds remain available for printers and APIs.");for(int i=0;i<nodeConfigUi.count;i++){auto&n=nodeConfigUi.nodes[i];auto*c=box(s,0,y,LIST_W,62,CARD,7);label(c,n.name,9,8,(LIST_W-31),INK);label(c,">",LIST_W-16,24,9,CYAN);label(c,deviceNodeType(n.type)?String(platformName(n.platform))+(localNodeType(n.type)?" • local":agentNodeType(n.type)?" • agent":" • feed")+(!n.enabled?" • paused":""):!strcmp(n.type,"local-proxmox")?String("Local Proxmox"):!strcmp(n.type,"klipper")?String("Klipper"):String("Remote Proxmox feed"),9,29,(LIST_W-26),MUTED,&lv_font_montserrat_10);label(c,n.id,9,45,(LIST_W-26),MUTED,&lv_font_montserrat_8);clickable(c,editNode,(void*)(uintptr_t)i);y+=68;}if(!nodeConfigUi.count)y=note(s,y,"No nodes configured. Add a monitor below.");if(nodeConfigUi.count<MAX_CONFIG_NODES&&activeConfiguredNodes(nodeConfigUi)<MAX_NODES&&activeConfiguredRoots(nodeConfigUi)<4){smallButton(s,"+ Add polling feed",0,y,LIST_W,CYAN,addNode);y+=48;}smallButton(s,"Refresh configuration",0,y,LIST_W,MUTED,refreshNodes);
}
void nodeForm(){
  bool local=localNodeType(editingNode.type),agent=agentNodeType(editingNode.type);heading(nodeExisting?"Edit node":"Add node","",true);auto*s=scroll(28);int y=0;if(formMessage.length())y=note(s,y,formMessage,RED);if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,nodeConfigUi.busy?CYAN:AMBER);if(!nodeExisting){const char*names[]={"Hub host","Klipper","PVE feed","Device feed"};for(int i=0;i<4;i++){const char*t=i==0?(!strcmp(nodeConfigUi.localType,"server")?"local-server":"local-proxmox"):i==1?"klipper":i==2?"proxmox-feed":"server-feed";int width=(LIST_W-6)/2;auto*b=box(s,(i%2)*(width+6),y+(i/2)*46,width,40,CARD,7);bool active=!strcmp(editingNode.type,t);lv_obj_set_style_border_color(b,color(active?CYAN:EDGE),0);label(b,names[i],7,13,width-14,active?CYAN:MUTED,&lv_font_montserrat_10);clickable(b,setNodeType,(void*)(uintptr_t)i);}y+=92;}if(local){const char*names[]={"Proxmox","Server"};for(int i=0;i<2;i++){int width=(LIST_W-6)/2;auto*b=box(s,i*(width+6),y,width,40,CARD,7);bool active=!strcmp(editingNode.type,i?"local-server":"local-proxmox");lv_obj_set_style_border_color(b,color(active?CYAN:EDGE),0);label(b,names[i],7,13,width-14,active?CYAN:MUTED,&lv_font_montserrat_10);clickable(b,setLocalNodeMode,(void*)(uintptr_t)i);}y+=48;y=note(s,y,String("Platform: ")+(editingNode.platform[0]?platformName(editingNode.platform):"Native host")+" • collector host");}if(deviceNodeType(editingNode.type)&&!local&&!agent){auto*b=box(s,0,y,LIST_W,40,CARD,7);label(b,String("Platform: ")+(editingNode.platform[0]?platformName(editingNode.platform):"Auto from device")+"  >",9,13,LIST_W-18,CYAN,&lv_font_montserrat_10);clickable(b,cycleNodePlatform);y+=48;}formField(s,y,nodeFields[0],nodeDraft[0].length()?nodeDraft[0]:String("Enter display name"),0);y+=54;
  if(editingNode.parentId[0])y=note(s,y,String("Subnode of: ")+editingNode.parentId);
  if(agent){y=note(s,y,"Device agent • "+String(platformName(editingNode.platform)));y=note(s,y,"Fixed ID: "+String(editingNode.id));y=note(s,y,editingNode.registered?"Paired. This device sends its readings to the collector.":"Pair or reconnect this device in the collector web console.");y=note(s,y,"Enrollment and credential renewal are available in the web console.");}else if(local){y=note(s,y,(deviceNodeType(editingNode.type)?String("Local device address: "):String("Local Proxmox address: "))+String(nodeConfigUi.localAddress));y=note(s,y,"Fixed ID: "+String(nodeConfigUi.localId));}else{formField(s,y,nodeFields[1],nodeDraft[1].length()?nodeDraft[1]:String(!strcmp(editingNode.type,"klipper")?"http://printer:7125":"http://collector:8765/api/v1/snapshot"),1);y+=54;formField(s,y,nodeFields[2],nodeDraft[2].length()?String("•••• (new service token)"):editingNode.hasSecret?String("Saved • keep for same address"):String(strcmp(editingNode.type,"klipper")?"Other collector display token":"Optional Moonraker API key"),2);y+=54;if(nodeExisting)y=note(s,y,"Fixed ID: "+String(editingNode.id));else{formField(s,y,nodeFields[3],nodeDraft[3].length()?nodeDraft[3]:String("Blank creates ID from name"),3);y+=54;}section(s,y,"ADVANCED SAMPLING");y+=18;for(int i=4;i<7;i++){formField(s,y,nodeFields[i],nodeDraft[i],i);y+=54;}y=note(s,y,"TTL covers at least two polls. Saved service tokens stay only for the same address. The collector validates all ranges.");if(editingNode.hasSecret){auto*c=box(s,0,y,LIST_W,40);label(c,nodeClearSecret?"Clear saved service token: ON":"Clear saved service token: OFF",9,13,(LIST_W-18),nodeClearSecret?AMBER:MUTED,&lv_font_montserrat_10);clickable(c,toggleNodeSecret);y+=46;}}
  if(nodeExisting){auto*c=box(s,0,y,LIST_W,40);label(c,editingNode.enabled?"Monitoring: ON":"Monitoring: PAUSED",9,13,LIST_W-18,editingNode.enabled?CYAN:AMBER,&lv_font_montserrat_10);clickable(c,toggleNodeEnabled);y+=46;}
  smallButton(s,nodeConfigUi.busy?"Saving...":"Save node",0,y,LIST_W,CYAN,saveNode);y+=48;if(nodeExisting){smallButton(s,"Remove node",0,y,LIST_W,RED,confirmDelete);y+=48;}note(s,y,"Saving briefly restarts the monitoring collector.");
}
void deleteScreen(){heading("Remove node?","",true);auto*s=scroll(28);int y=note(s,0,editingNode.name,INK);y=note(s,y,String(deleteNodeId));y=note(s,y,"Remove this node from desk monitoring? You can add it again from Settings.");if(formMessage.length())y=note(s,y,formMessage,RED);if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,AMBER);smallButton(s,nodeConfigUi.busy?"Removing...":"Remove this node",0,y,LIST_W,RED,deleteConfirmed);y+=48;smallButton(s,"Keep node",0,y,LIST_W,CYAN,cancelDelete);}
void openFlipper(lv_event_t*){flipper::readState(flipperUi);openPage(FLIPPER_PAGE);}
void flipperToggle(lv_event_t*){flipper::setEnabled(!flipperUi.enabled);flipper::readState(flipperUi);render();}
void flipperPair(lv_event_t*){flipper::pair();flipper::readState(flipperUi);render();}
void flipperForget(lv_event_t*){flipper::forget();flipper::readState(flipperUi);render();}
void flipperEnterPin(lv_event_t*){flipperPinDraft="";fieldOriginal="";fieldOrigin=FLIPPER_PAGE;fieldIndex=0;page=18;render();}
void flipperConfirm(lv_event_t*){flipper::confirmPairing(true);}
void flipperCancel(lv_event_t*){flipper::confirmPairing(false);}
void flipperScreen(){
  heading("Flipper status screen","",true);auto*s=scroll(28);int y=0;
  y=note(s,y,flipperUi.message[0]?String(flipperUi.message):String("Bluetooth status display is off"),flipperUi.connected?MINT:AMBER);
  if(flipperUi.peer[0])y=note(s,y,String("Flipper: ")+flipperUi.peer,INK);
  if(flipperUi.pairing&&flipperUi.passkeyEntry){
    y=note(s,y,"Read the six-digit code on Flipper and enter it here to pair securely.",INK);
    smallButton(s,"Enter Flipper PIN",0,y,LIST_W,MINT,flipperEnterPin);y+=48;
    smallButton(s,"Cancel pairing",0,y,LIST_W,AMBER,flipperCancel);y+=48;
  }else if(flipperUi.pairing){
    char pin[7];snprintf(pin,sizeof(pin),"%06lu",(unsigned long)flipperUi.passkey);
    y=note(s,y,String("Pairing code: ")+pin,INK);
    y=note(s,y,"Compare this code with the Flipper. Confirm on both devices only when they match.");
    smallButton(s,"Confirm numbers match",0,y,LIST_W,MINT,flipperConfirm);y+=48;
    smallButton(s,"Cancel pairing",0,y,LIST_W,AMBER,flipperCancel);y+=48;
  }else{
    smallButton(s,flipperUi.enabled?"Bluetooth display: ON":"Bluetooth display: OFF",0,y,LIST_W,CYAN,flipperToggle);y+=48;
    smallButton(s,"Pair / reconnect Flipper",0,y,LIST_W,MINT,flipperPair);y+=48;
    if(flipperUi.peer[0]){smallButton(s,"Forget Flipper",0,y,LIST_W,AMBER,flipperForget);y+=48;}
  }
  y=note(s,y,"Open the Glimdock app on Flipper to start its Bluetooth status service, then tap Pair here. Disconnect the phone app before pairing.");
  y=note(s,y,"Flipper Up/Down selects a node. Left/Right switches System, Alerts, Agents and Printer pages. OK returns to System; Back exits.");
  note(s,y,"System and Alerts follow the selected Glimdock node. Updates arrive once per second; stale readings stay marked.");
}
void settings(){
  auto*s=scroll();int y=0;section(s,y,"DISPLAY SETTINGS");y+=20;smallButton(s,"Private conversations",0,y,LIST_W,LAVENDER,privateOpen);y+=48;
  smallButton(s,"Flipper Bluetooth screen",0,y,LIST_W,CYAN,openFlipper);y+=48;
  smallButton(s,soundEnabled?"Notification sounds: ON":"Notification sounds: OFF",0,y,LIST_W,MINT,soundChanged);y+=46;
  auto*volumeCard=box(s,0,y,LIST_W,68);label(volumeCard,"Sound volume",10,10,LIST_W-68,INK,&lv_font_montserrat_14);auto*volumeLabel=label(volumeCard,String(soundVolume)+"%",LIST_W-57,12,47,MUTED,&lv_font_montserrat_12);lv_obj_set_style_text_align(volumeLabel,LV_TEXT_ALIGN_RIGHT,0);
  auto*volumeSlider=lv_slider_create(volumeCard);lv_obj_add_flag(volumeSlider,LV_OBJ_FLAG_PRESS_LOCK);lv_obj_set_pos(volumeSlider,14,43);lv_obj_set_size(volumeSlider,LIST_W-30,8);lv_obj_set_ext_click_area(volumeSlider,16);lv_slider_set_range(volumeSlider,0,100);lv_slider_set_value(volumeSlider,soundVolume,LV_ANIM_OFF);lv_obj_set_style_bg_color(volumeSlider,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(volumeSlider,color(CYAN),LV_PART_INDICATOR);lv_obj_set_style_bg_color(volumeSlider,color(CYAN),LV_PART_KNOB);for(auto part:{LV_PART_MAIN,LV_PART_INDICATOR,LV_PART_KNOB})lv_obj_set_style_border_width(volumeSlider,0,part);lv_obj_add_event_cb(volumeSlider,volumeChanged,LV_EVENT_VALUE_CHANGED,volumeLabel);lv_obj_add_event_cb(volumeSlider,volumeChanged,LV_EVENT_RELEASED,volumeLabel);y+=76;
  if(boardAudioReady()){smallButton(s,"Test speaker",0,y,LIST_W,MUTED,soundTest);y+=46;}else y=note(s,y,"Speaker unavailable",AMBER);
  auto*batteryCard=box(s,0,y,LIST_W,72);label(batteryCard,"Device battery",10,8,LIST_W-20,INK,&lv_font_montserrat_12);deviceBatteryLabel=label(batteryCard,"",10,30,LIST_W-20,MUTED,&lv_font_montserrat_14);label(batteryCard,"3.7 V LiPo • approximate voltage",10,54,LIST_W-20,MUTED,&lv_font_montserrat_8);y+=80;updateBatteryLabels();y=note(s,y,"Level varies with load and charging. Charge status and remaining runtime are unavailable.");
  smallButton(s,"Open clock mode",0,y,LIST_W,LAVENDER,clockGo);y+=46;auto*zone=box(s,0,y,LIST_W,40,CARD,7);label(zone,String("Clock time zone: ")+displayClockZoneName(clockZone),9,13,LIST_W-26,INK,&lv_font_montserrat_10);label(zone,">",LIST_W-17,13,10,LAVENDER,&lv_font_montserrat_10);clickable(zone,clockZoneChanged);y+=46;
  auto*c=box(s,0,y,LIST_W,PORTRAIT?76:48);label(c,"Appearance",10,PORTRAIT?8:16,98,INK);const char*choices[]={"White","Dark","Auto"};int choiceW=PORTRAIT?(LIST_W-28)/3:58,choiceX=PORTRAIT?10:110;for(int i=0;i<3;i++){bool active=themeMode==i;auto*b=box(c,choiceX+i*(choiceW+4),PORTRAIT?28:4,choiceW,40,active?(lightTheme?CARD:DARK_SELECTED):CARD,7);if(active)lv_obj_set_style_border_color(b,color(CYAN),0);auto*l=label(b,choices[i],4,13,choiceW-8,active?CYAN:MUTED,&lv_font_montserrat_12);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_CENTER,0);clickable(b,themeChanged,(void*)(uintptr_t)i);}y+=PORTRAIT?84:56;if(themeMode==THEME_AUTO){struct tm local{};String hint="Auto: light 07:00-19:00 • "+String(displayClockZoneName(clockZone));if(!displayClockTm(time(nullptr),clockZone,local))hint+="\nWaiting for clock sync; keeping current appearance.";y=note(s,y,hint);}
  c=box(s,0,y,LIST_W,68);label(c,"Brightness",10,10,LIST_W-20,INK,&lv_font_montserrat_14);auto*slider=lv_slider_create(c);lv_obj_add_flag(slider,LV_OBJ_FLAG_PRESS_LOCK);lv_obj_set_pos(slider,14,43);lv_obj_set_size(slider,(LIST_W-30),8);lv_obj_set_ext_click_area(slider,16);lv_slider_set_range(slider,10,100);lv_slider_set_value(slider,brightness,LV_ANIM_OFF);lv_obj_set_style_bg_color(slider,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(slider,color(CYAN),LV_PART_INDICATOR);lv_obj_set_style_bg_color(slider,color(CYAN),LV_PART_KNOB);lv_obj_set_style_border_width(slider,0,LV_PART_MAIN);lv_obj_set_style_border_width(slider,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(slider,0,LV_PART_KNOB);lv_obj_add_event_cb(slider,brightnessChanged,LV_EVENT_RELEASED,nullptr);y+=76;
  c=box(s,0,y,LIST_W,48);label(c,"Rotate pages every 15s",10,15,LIST_W-72,INK,PORTRAIT?&lv_font_montserrat_10:&lv_font_montserrat_12);auto*sw=lv_switch_create(c);lv_obj_set_pos(sw,LIST_W-54,12);lv_obj_set_size(sw,44,24);lv_obj_remove_flag(sw,LV_OBJ_FLAG_CHECKABLE);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(sw,color(CARD),LV_PART_KNOB);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_INDICATOR);lv_obj_set_style_bg_color(sw,color(CYAN),LV_PART_INDICATOR|LV_STATE_CHECKED);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_MAIN);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_INDICATOR);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_KNOB);lv_obj_set_style_border_width(sw,0,LV_PART_MAIN);lv_obj_set_style_border_width(sw,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(sw,0,LV_PART_KNOB);if(autoRotate)lv_obj_add_state(sw,LV_STATE_CHECKED);clickable(sw,rotateChanged);clickable(c,rotateChanged,sw);y+=56;
  c=box(s,0,y,LIST_W,48);label(c,"Dim after 2 min",10,15,LIST_W-72,INK);sw=lv_switch_create(c);lv_obj_set_pos(sw,LIST_W-54,12);lv_obj_set_size(sw,44,24);lv_obj_remove_flag(sw,LV_OBJ_FLAG_CHECKABLE);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(sw,color(CARD),LV_PART_KNOB);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_INDICATOR);lv_obj_set_style_bg_color(sw,color(CYAN),LV_PART_INDICATOR|LV_STATE_CHECKED);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_MAIN);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_INDICATOR);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_KNOB);lv_obj_set_style_border_width(sw,0,LV_PART_MAIN);lv_obj_set_style_border_width(sw,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(sw,0,LV_PART_KNOB);if(idleDim)lv_obj_add_state(sw,LV_STATE_CHECKED);clickable(sw,dimChanged);clickable(c,dimChanged,sw);y+=56;y=note(s,y,"Touch wakes when dimmed. Off stays bright.");smallButton(s,"Nodes & monitoring",0,y,LIST_W,CYAN,openNodeSettings);y+=48;smallButton(s,"Wi-Fi & collector setup",0,y,LIST_W,CYAN,openConnection);y+=48;
  if(manualDemo){smallButton(s,"Exit demo",0,y,LIST_W,AMBER,exitDemo);y+=48;}else{smallButton(s,"Preview demo (sample data)",0,y,LIST_W,LAVENDER,startDemo);y+=48;}
  row(s,y,"Feed status",feedStatus,feedStatusColor);feedStatusLabel=lv_obj_get_child(lv_obj_get_child(s,lv_obj_get_child_count(s)-1),1);y+=44;row(s,y,"Display IP",net.ip[0]?String(net.ip):String("Not connected"));y+=44;row(s,y,"Network",net.message);y+=44;section(s,y,"Glimdock • open desk monitor",LAVENDER);y+=20;note(s,y,"Waveshare ESP32-S3 Touch LCD 2.8 V1 / schema 1");
}
#include "operations_ui.inc"
#include "private_chat_ui.inc"
void setupScreen(){
  auto*s=scroll();label(s,"SET UP GLIMDOCK",4,2,LIST_W-4,LAVENDER,&lv_font_montserrat_14);int y=note(s,25,net.setup?String("Join the Wi-Fi network below"):net.message[0]?String(net.message):String("Starting configuration network..."));smallButton(s,"Configure on this display",0,y,LIST_W,CYAN,openConnection);y+=48;
  row(s,y,"Wi-Fi network",glimdock::SETUP_SSID,CYAN);y+=44;row(s,y,"Setup password",net.apPassword[0]?String(net.apPassword):String("Preparing..."),INK);y+=44;row(s,y,"Open browser","192.168.4.1",MINT);y+=44;
  y=note(s,y,"Enter your Wi-Fi credentials, collector URL and dedicated display token. Any compatible collector can provide your configured nodes.");smallButton(s,"Preview demo (sample data)",0,y,LIST_W,LAVENDER,startDemo);
}
void noData(){
  // Error text can span several lines; keep every action reachable by scrolling.
  auto*s=scroll();auto*c=box(s,0,0,LIST_W,CONTENT_H-6);int textW=LIST_W-24;
  String title=net.loading?"LOADING NODE":net.wifi?"WAITING FOR COLLECTOR":"CONNECTING";
  lv_point_t textSize;displayTextSize(&textSize,title,&lv_font_montserrat_18,0,0,textW,LV_TEXT_FLAG_NONE);
  auto*l=label(c,title,12,12,textW,CYAN,&lv_font_montserrat_18);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);int y=12+textSize.y+12;
  String message=net.message[0]?String(net.message):String("Waiting for network worker");displayTextSize(&textSize,message,&lv_font_montserrat_12,0,0,textW,LV_TEXT_FLAG_NONE);
  l=label(c,message,12,y,textW,MUTED);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=textSize.y+12;
  smallButton(c,"Wi-Fi & collector setup",12,y,textW,CYAN,openConnection);y+=48;label(c,"No live readings received yet",12,y,textW,MUTED,&lv_font_montserrat_10);lv_obj_set_height(c,y+lv_font_get_line_height(&lv_font_montserrat_10)+12);
}
void render(){
  if(!uiReady)return;applyAppearance();struct tm appearanceTime{};appearanceClockSynced=themeMode==THEME_AUTO&&displayClockTm(time(nullptr),clockZone,appearanceTime);readManagement();if(keyboard){lv_obj_delete(keyboard);keyboard=nullptr;fieldArea=nullptr;}if(page==18)lv_obj_add_flag(tabBar,LV_OBJ_FLAG_HIDDEN);else lv_obj_remove_flag(tabBar,LV_OBJ_FLAG_HIDDEN);int oldScroll=privateNewRequest?0:scrollArea?lv_obj_get_scroll_y(scrollArea):0;static uint8_t previousPage=255;feedStatusLabel=nullptr;deviceBatteryLabel=nullptr;privateWipeLabels(content);lv_obj_clean(content);workspaceHighlight=nullptr;scrollArea=nullptr;header();
  if(page==FLIPPER_PAGE)flipperScreen();else if(privatePage(page))privateRender();else if(operationsPage(page))operationsRender();else if(page==18)fieldEditor();else if(page==17)deleteScreen();else if(page==16)nodeForm();else if(page==15)nodeManager();else if(page==14)connectionForm();else if(page==13)nodePicker();else if(page==8)setupScreen();else if(page==7)settings();else if(page==ALL_NODES_PAGE)nodesOverview();else if(page==CLOCK_PAGE)clockOverview();else if(page==SUBNODES_PAGE)subnodesList();else if(!snapshot->valid)noData();else if(printerNode()){switch(page){case 1:printerJob();break;case 2:printerTemps();break;case 3:case 6:printerHealth();break;default:printerOverview();break;}}else switch(page){case 0:overview();break;case 1:guests();break;case 2:storage();break;case 3:sensors();break;case 4:guestDetail();break;case 5:powerDetail();break;case 6:alerts();break;case 9:diskDetail();break;case 10:memoryDetail();break;case 11:gpuList();break;case 12:gpuDetail();break;}
  if(scrollArea&&previousPage==page)lv_obj_scroll_to_y(scrollArea,oldScroll,LV_ANIM_OFF);if(scrollArea&&page==PRIVATE_MESSAGES&&privateScrollToBottom&&privateChat&&privateChat->valid&&!privateState.loading){lv_obj_update_layout(scrollArea);lv_obj_scroll_to_y(scrollArea,lv_obj_get_scroll_y(scrollArea)+lv_obj_get_scroll_bottom(scrollArea),LV_ANIM_OFF);privateScrollToBottom=false;}lv_obj_update_layout(content);lv_obj_stop_scroll_anim(content);lv_obj_scroll_to_x(content,0,LV_ANIM_OFF);lv_obj_scroll_to_y(content,0,LV_ANIM_OFF);lv_obj_invalidate(content);previousPage=page;privateNewRequest=false;lastRender=millis();styleNotification();
}
void buildUI(){
  lv_obj_t*screen=lv_screen_active();lv_obj_set_style_bg_color(screen,color(BG),0);lv_obj_set_style_pad_all(screen,0,0);lv_obj_remove_flag(screen,LV_OBJ_FLAG_SCROLLABLE);
  privateChat=(privatechat::Model*)heap_caps_malloc(sizeof(privatechat::Model),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);if(privateChat)new(privateChat)privatechat::Model{};
  operations=(ops::Snapshot*)heap_caps_malloc(sizeof(ops::Snapshot),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);if(operations)new(operations)ops::Snapshot{};
  int homeH=PORTRAIT?26:HEADER_H;auto*home=box(screen,0,0,104,homeH,BG,0);lv_obj_set_style_bg_opa(home,LV_OPA_TRANSP,0);headerMark=brandMark(home,4,PORTRAIT?4:8);headerTitle=brandWordmark(home,27,PORTRAIT?0:4);clickable(home,workspaceHome);
  // The wordmark's existing dot carries status; its hit area draws nothing.
  brandStatusButton=box(home,86,PORTRAIT?8:12,18,18,BG,0);lv_obj_set_style_bg_opa(brandStatusButton,LV_OPA_TRANSP,0);lv_obj_set_style_border_width(brandStatusButton,0,0);clickable(brandStatusButton,feedStatusGo,(void*)6);
  // Balance the visible gaps around the time, rather than the wider touch boxes.
  int clockW=PORTRAIT?60:52,clockX=VIEW_W-(PORTRAIT?8:34)-clockW,nodeX=PORTRAIT?8:116,nodeW=PORTRAIT?156:104;
  nodeButton=box(screen,nodeX,PORTRAIT?27:4,nodeW,26,CARD,7);headerHost=label(nodeButton,"Choose node",8,PORTRAIT?6:7,nodeW-34,MUTED,PORTRAIT?&lv_font_montserrat_12:&lv_font_montserrat_10);label(nodeButton,LV_SYMBOL_DOWN,nodeW-20,8,12,CYAN,&lv_font_montserrat_10);clickable(nodeButton,chooseNode);
  clockButton=box(screen,clockX,PORTRAIT?27:0,clockW,PORTRAIT?26:HEADER_H,BG,0);headerClock=label(clockButton,"--:--",2,PORTRAIT?0:3,clockW-4,INK,&lv_font_montserrat_12);lv_obj_set_style_text_align(headerClock,LV_TEXT_ALIGN_CENTER,0);headerBattery=label(clockButton,"-- V",2,PORTRAIT?15:21,clockW-4,MUTED,&lv_font_montserrat_8);lv_obj_set_style_text_align(headerBattery,LV_TEXT_ALIGN_CENTER,0);clickable(clockButton,clockGo);
  settingsButton=box(screen,VIEW_W-34,0,34,PORTRAIT?26:HEADER_H,BG,0);auto*gear=label(settingsButton,LV_SYMBOL_SETTINGS,10,PORTRAIT?6:10,18,MUTED,&lv_font_montserrat_14);if(!PORTRAIT)lv_obj_align(gear,LV_ALIGN_CENTER,2,0);clickable(settingsButton,go,(void*)7);headerLine=box(screen,0,HEADER_H-1,VIEW_W,1,EDGE,0);lv_obj_set_style_border_width(headerLine,0,0);
  content=box(screen,0,HEADER_H,VIEW_W,CONTENT_H,BG,0);
  tabBar=box(screen,0,VIEW_H-TAB_H,VIEW_W,TAB_H,CARD,0);lv_obj_set_style_border_side(tabBar,LV_BORDER_SIDE_TOP,0);
  for(int i=0;i<4;i++){tabs[i]=box(tabBar,i*TAB_W,0,TAB_W,TAB_H,CARD,0);lv_obj_set_style_border_width(tabs[i],0,0);icon(tabs[i],16+i,(TAB_W-13)/2,6,13,MUTED);auto*l=label(tabs[i],titles[i],2,23,TAB_W-4,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_CENTER,0);auto*rule=box(tabs[i],(TAB_W-26)/2,0,26,2,CYAN,0);lv_obj_set_style_border_width(rule,0,0);clickable(tabs[i],tabGo,(void*)(uintptr_t)i);}
  uiReady=true;render();
}
void captureHistory(){for(int i=0;i<31;i++){cpuHistory[i]=cpuHistory[i+1];ramHistory[i]=ramHistory[i+1];}cpuHistory[31]=snapshot->cpu;ramHistory[31]=memoryPct(snapshot->memUsed,snapshot->memTotal);historyCount=min(32,int(historyCount+1));}
void processAudioNotices(uint32_t now){
  if(!snapshot->valid||demo()||stale()||net.loading)return;
  static uint64_t lastSequence=UINT64_MAX;static bool connected=false;static uint32_t lastNotice=0;
  if(snapshot->sequence==lastSequence)return;lastSequence=snapshot->sequence;
  if(!connected){connected=true;boardAudioPlay(AudioCue::Connected);showNotification("Connected","Live homelab readings are ready",3);}
  uint64_t keys[MAX_ALERTS];uint8_t alertIndices[MAX_ALERTS];size_t count=0;
  for(int i=0;i<snapshot->nAlerts;i++){const auto&a=snapshot->alerts[i];if(!strcmp(a.severity,"warning")||!strcmp(a.severity,"critical")){alertIndices[count]=i;keys[count++]=NotificationPolicy::alertKey(a.id[0]?a.id:a.message,a.severity);}}
  bool warning=snapshot->node.id[0]&&notices.alertsChanged(snapshot->node.id,keys,count),complete=false;
  if(printerNode()&&printerFresh())complete=notices.printerChanged(snapshot->node.id,snapshot->printer.state);String completedPrinter=complete?String(snapshot->node.name):String("");
  for(int i=0;i<snapshot->nNodes;i++){const auto&n=snapshot->nodes[i];if(strcmp(n.id,snapshot->node.id)&&!strcmp(n.type,"klipper")&&nodeSummaryFresh(n)&&notices.printerChanged(n.id,n.summary.printState)){complete=true;completedPrinter=n.name;}}
  if((warning||complete)&&(!lastNotice||uint32_t(now-lastNotice)>=10000)){lastNotice=now?now:1;boardAudioPlay(warning?AudioCue::Alert:AudioCue::PrintComplete);if(warning){const auto&a=snapshot->alerts[alertIndices[notices.newAlert]];bool critical=!strcmp(a.severity,"critical");showNotification(critical?"Critical alert":"New alert",String(snapshot->node.name)+": "+a.message,critical?2:1);}else showNotification("Print complete",completedPrinter+" • your print has finished",3);}
}
void processBatteryNotice(uint32_t now){
  auto battery=boardBattery();
  if(batteryNotices.update(battery,now))lowBatteryNoticePending=true;
  bool low=battery.available()&&(battery.level==BatteryLevel::Low||battery.level==BatteryLevel::VeryLow);
  // Do not replace another banner or interrupt an active touch/text edit.
  if(!low)lowBatteryNoticePending=false;
  if(!lowBatteryNoticePending||notification||interactionBusy()||page==18)return;
  lowBatteryNoticePending=false;boardAudioPlay(AudioCue::Alert);
  showNotification("Battery low",String("Device ")+batteryVoltage(battery)+" • connect USB power",battery.level==BatteryLevel::VeryLow?2:1);
  Serial.printf("Device battery: low-voltage notice at %.2f V; quiet until sustained recovery\n",battery.volts);
}
void publishFlipperStatus(uint32_t now,bool urgent=false){
  lastFlipperPublish=now;GdStatus status;uint8_t wire[GD_DETAIL_BYTES];
  flipperSequence=(flipperSequence+1)&0x7fffffffU;if(!flipperSequence)flipperSequence=1;
  const double epoch=double(time(nullptr));
  flipper::buildStatus(*snapshot,net,operations,operationsNet,now,epoch,flipperSequence,status);
  const uint32_t request=!strcmp(snapshot->node.id,flipperRequestedNode)?flipperRequestSequence:0;
  if(gd_detail_encode(&status,snapshot->node.id,request,snapshot->valid&&!net.loading,wire,sizeof(wire)))
    flipper::publish(wire,sizeof(wire),urgent);
  // Static storage avoids placing the bounded fleet inventory on the UI stack.
  static GdNodeCatalog catalog;
  memset(&catalog,0,sizeof(catalog));
  strlcpy(catalog.selected_id,snapshot->node.id,sizeof(catalog.selected_id));
  for(unsigned i=0;i<snapshot->nNodes&&i<MAX_NODES&&catalog.count<GD_CATALOG_MAX_NODES;++i){
    const auto&node=snapshot->nodes[i];if(!validNodeId(node.id))continue;
    auto&entry=catalog.nodes[catalog.count++];strlcpy(entry.id,node.id,sizeof(entry.id));
    flipper::buildNodeSummary(node,*snapshot,net,now,epoch,entry.system);
  }
  if(catalog.count||(!net.loading&&net.configured))flipper::publishCatalog(catalog);
}
void updateUi(){
  if(!snapshot)return;uint32_t now=millis();
  if(applyFlipperNodeSelection()){
    publishFlipperStatus(now,true);
    if(page!=18&&!interactionBusy())render();else header();
  }
  if(now-lastPoll>=500){lastPoll=now;if(privatePage(page)&&privateChat)readPrivateChat(*privateChat,privateState);if(operations)readOperations(*operations,operationsNet);if(manualDemo)makeDemo(*snapshot);else readSnapshot(*snapshot,net);
    if(strcmp(trackedNodeId,snapshot->node.id)||(net.loading&&!snapshot->valid&&lastSequence))resetNodeView(true);
    if(!net.configured&&!manualDemo&&page!=7&&(page<13||page==ALL_NODES_PAGE))page=8;
    else if(net.configured&&!net.setup&&page==8&&!manualDemo)page=ALL_NODES_PAGE;
    if(snapshot->valid&&!printerNode()&&(manualDemo||snapshot->sequence!=lastSequence)){captureHistory();lastSequence=snapshot->sequence;}
    bool changed=net.revision!=lastNetRevision;lastNetRevision=net.revision;
    uint32_t previousFlipperRevision=flipperUi.revision;
    flipper::readState(flipperUi);
    bool flipperChanged=previousFlipperRevision!=flipperUi.revision;
    if(page==18&&fieldOrigin==FLIPPER_PAGE&&!flipperUi.pairing){flipperPinDraft="";fieldOriginal="";page=FLIPPER_PAGE;render();}
    if(flipperUi.enabled&&uint32_t(now-lastFlipperPublish)>=1000){
      publishFlipperStatus(now);
    }
    processAudioNotices(now);
    processBatteryNotice(now);
    bool appearanceChanged=false;if(themeMode==THEME_AUTO&&page!=18&&now-lastTouch>500&&!interactionBusy()){struct tm local{};bool synced=displayClockTm(time(nullptr),clockZone,local);appearanceChanged=applyAppearance()||synced!=appearanceClockSynced;}if(appearanceChanged||(page==FLIPPER_PAGE&&flipperChanged&&!interactionBusy()))render();else if(page!=7&&page!=18&&now-lastTouch>500&&!interactionBusy()&&(changed||now-lastRender>=1000))render();else header();
  }
  if(autoRotate&&pageTab(page)>=0&&snapshot->valid&&!interactionBusy()&&now-lastRotate>HOMELAB_AUTO_PAGE_MS){openPage(tabPage((pageTab(page)+1)%4));lastRotate=now;}
  if(idleDim&&!dimmed&&now-lastTouch>HOMELAB_IDLE_DIM_MS){dimmed=true;boardBacklight(10);}
  if(notification&&uint32_t(millis()-notificationStarted)>=4500&&!interactionBusy())dismissNotification();
}
}
void noteTouch(bool pressed){if(!uiReady)return;if(pressed){lastTouch=millis();lastRotate=millis();if(dimmed){dimmed=false;consumeWakeTouch=true;boardBacklight(brightness);}}else if(consumeWakeTouch)consumeWakeTouch=false;}
#ifndef HOMELAB_NATIVE_PREVIEW
void setup(){
  boardHoldPower();
  size_t privateRx=Serial.setRxBufferSize(privatechat::MAX_CONFIG_JSON+64);Serial.begin(115200);if(!privateRx)Serial.println("Private reader USB receive buffer unavailable.");boardInit();boardLvglInit();Preferences p;p.begin("display",true);brightness=constrain(p.getUChar("brightness",80),10,100);autoRotate=p.getBool("rotate",false);idleDim=p.getBool("dim",false);soundEnabled=p.getBool("sound",true);soundVolume=clampSoundVolume(p.getUChar("volume",DEFAULT_SOUND_VOLUME));loadAppearance(p);agentsWorkspace=p.getBool("agentsMode",false);if(agentsWorkspace)page=OPS_OVERVIEW;p.end();boardAudioVolume(soundVolume);boardAudioInit(soundEnabled);
  Serial.printf("Glimdock boot: reset=%d; power latch=on; gyro/RTC=off; notification audio=%s\n",int(esp_reset_reason()),boardAudioReady()?"ready":"unavailable");
  startRuntimeHealth();
  snapshot=(Snapshot*)heap_caps_malloc(sizeof(Snapshot),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
  if(!snapshot){auto*l=lv_label_create(lv_screen_active());lv_label_set_text(l,"PSRAM unavailable\nCheck V1 board configuration");lv_obj_center(l);boardBacklight(80);return;}new(snapshot)Snapshot{};
  for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}initPrivateChat();startNetwork();flipper::begin();lastTouch=lastRotate=millis();buildUI();boardBacklight(brightness);
}
void loop(){
  tickRuntimeHealth();privateChatProvisionTick();int privateView=consumePrivateChatViewRequest();if(privateView==1&&uiReady)privateOpen(nullptr);else if(privateView==2&&uiReady&&privatePage(page))openPage(7);else if(privateView==3&&uiReady&&page==PRIVATE_SESSIONS)privateSelectIndex(0);
  if(!snapshot){lv_timer_handler();delay(10);return;}updateUi();
  lv_timer_handler();delay(5);
}

#else
void homelabPreviewFlipperState(const flipper::State& state){flipper::previewState()=state;flipperUi=state;openPage(FLIPPER_PAGE);}
void homelabPreviewFlipperPinEditor(){flipperEnterPin(nullptr);}
void homelabPreviewInit(Snapshot* data,NetworkState* state){initPrivateChat();snapshot=data;net=*state;strlcpy(trackedNodeId,data->node.id,sizeof(trackedNodeId));brightness=80;lastTouch=millis();lastRotate=millis();for(int i=0;i<32;i++){cpuHistory[i]=data->demo?16+i%8:NAN;ramHistory[i]=data->demo?55+i%4:NAN;}cpuHistory[31]=data->cpu;ramHistory[31]=memoryPct(data->memUsed,data->memTotal);buildUI();}
void homelabPreviewShow(uint8_t selected){if(operations)readOperations(*operations,operationsNet);agentsWorkspace=operationsPage(selected)||privatePage(selected);if(selected==PRIVATE_SESSIONS){privateOpen(nullptr);return;}if(selected==14){openConnection(nullptr);return;}if(selected==15){openNodeSettings(nullptr);return;}if(selected==16||selected==17){readManagement();if(selected==16&&!nodeConfigUi.count){addNode(nullptr);return;}ConfigNode n=nodeConfigUi.count>1?nodeConfigUi.nodes[1]:nodeConfigUi.count?nodeConfigUi.nodes[0]:ConfigNode{};beginNode(n,nodeConfigUi.count>0);if(selected==17){strlcpy(deleteNodeId,n.id,sizeof(deleteNodeId));page=17;render();}return;}if(selected==18){openConnection(nullptr);fieldOrigin=14;fieldIndex=4;fieldOriginal=wifiDraft[fieldIndex];page=18;render();return;}if(selected==11)gpuListOrigin=5;if(selected==4&&snapshot->nGuests)selectedGuestId=snapshot->guests[0].id;if(selected==9&&snapshot->nDisks)strlcpy(selectedDiskName,snapshot->disks[0].name,sizeof(selectedDiskName));if(selected==12&&snapshot->nGpus){strlcpy(selectedGpuId,snapshot->gpus[0].id,sizeof(selectedGpuId));gpuReturnPage=11;}detailOrigin=selected==4?1:selected==9?2:0;setViewPage(selected);render();}
void homelabPreviewGuest(int id){selectedGuestId=id;detailOrigin=1;page=4;render();}
void homelabPreviewRefresh(){render();}
void homelabPreviewPoll(){if(privatePage(page)&&privateChat)readPrivateChat(*privateChat,privateState);readSnapshot(*snapshot,net);if(operations)readOperations(*operations,operationsNet);if(strcmp(trackedNodeId,snapshot->node.id)||(net.loading&&!snapshot->valid&&lastSequence)){resetNodeView(true);}render();}
void homelabPreviewTick(){updateUi();}
bool homelabPreviewNotificationVisible(){return notification!=nullptr;}
void homelabPreviewNotification(const char*title,const char*message,uint8_t kind){showNotification(title,message,kind);}
int homelabPreviewHeaderHeight(){return HEADER_H;}
uint8_t homelabPreviewHistoryCount(){return historyCount;}
void homelabPreviewGpu(uint8_t index){if(index<snapshot->nGpus)strlcpy(selectedGpuId,snapshot->gpus[index].id,sizeof(selectedGpuId));gpuReturnPage=11;detailOrigin=0;page=12;render();}
void homelabPreviewScroll(int y){if(scrollArea)lv_obj_scroll_to_y(scrollArea,y,LV_ANIM_OFF);}
void homelabPreviewSensorFilter(uint8_t kind){sensorFilter=min(5,int(kind));page=3;render();}
void homelabPreviewTheme(bool light){themeMode=light?THEME_LIGHT:THEME_DARK;useTheme(light);render();}
uint8_t homelabPreviewThemeMode(){return themeMode;}
void homelabPreviewReloadAppearance(){Preferences p;if(p.begin("display",true)){loadAppearance(p);p.end();}render();}
uint8_t homelabPreviewPage(){return page;}
lv_obj_t*homelabPreviewStatusControl(){return brandStatusButton;}
uint8_t homelabPreviewFilter(){return sensorFilter;}
bool homelabPreviewLightTheme(){return lightTheme;}
bool homelabPreviewBusy(){return interactionBusy();}
bool homelabPreviewAutoRotate(){return autoRotate;}
bool homelabPreviewIdleDim(){return idleDim;}
uint8_t homelabPreviewBrightness(){return brightness;}
#endif
