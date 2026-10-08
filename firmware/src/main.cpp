#include <Arduino.h>
#include <Preferences.h>
#include <esp_heap_caps.h>
#include <lvgl.h>
#include <time.h>
#include <stdlib.h>
#include "board.h"
#include "model.h"
#include "config.h"
#include "brand.h"
#include "brand_assets.h"
bool consumeWakeTouch=false;
namespace {
struct Palette {uint32_t bg,card,ink,muted,cyan,lavender,mint,amber,red,edge;};
constexpr Palette LIGHT={0xf2f5f8,0xffffff,0x101e30,0x526275,0x007b96,0x6047ba,0x00734c,0x8a5300,0xb32632,0xdce5ed};
constexpr Palette DARK={0x080f18,0x121f2f,0xeaf2fb,0x8ba0b6,0x54d9f2,0xad9aff,0x64e5b0,0xffbe68,0xff7184,0x253449};
uint32_t BG=LIGHT.bg,CARD=LIGHT.card,INK=LIGHT.ink,MUTED=LIGHT.muted,CYAN=LIGHT.cyan,LAVENDER=LIGHT.lavender,MINT=LIGHT.mint,AMBER=LIGHT.amber,RED=LIGHT.red,EDGE=LIGHT.edge;
Snapshot *snapshot=nullptr;NetworkState net;
lv_obj_t *content=nullptr,*scrollArea=nullptr,*headerTitle=nullptr,*headerMark=nullptr,*headerHost=nullptr,*nodeButton=nullptr,*headerState=nullptr,*headerLine=nullptr,*statePill=nullptr,*settingsButton=nullptr,*tabBar=nullptr,*tabs[4]{};
uint8_t page=0,detailOrigin=0,brightness=80;int selectedGuestId=0;char selectedDiskName[48]{};bool autoRotate=false,idleDim=false,dimmed=false,manualDemo=false,uiReady=false,lightTheme=true;
char selectedGpuId[64]{},gpuLinkIds[MAX_GPUS][64]{},nodeLinkIds[MAX_NODES][64]{},trackedNodeId[64]{};uint8_t pickerOrigin=0;uint8_t gpuReturnPage=11,gpuListOrigin=5;
uint32_t lastTouch=0,lastRender=0,lastPoll=0,lastRotate=0,lastNetRevision=0;uint64_t lastSequence=0;
float cpuHistory[32]{},ramHistory[32]{};uint8_t historyCount=0;
ConnectionSettings connectionUi;NodeConfiguration nodeConfigUi;String wifiDraft[5],nodeDraft[7],formMessage,fieldOriginal;ConfigNode editingNode;bool wifiClearPassword=false,nodeExisting=false,nodeClearSecret=false,nodeSavePending=false,nodeDeletePending=false,fieldOriginalToggle=false,wifiSavePending=false;char deleteNodeId[64]{};uint8_t fieldOrigin=14,fieldIndex=0;lv_obj_t*keyboard=nullptr,*fieldArea=nullptr;
const char*const wifiFields[]={"Wi-Fi name","Wi-Fi password","Collector endpoint","Display token","Setup token"};
const char*const nodeFields[]={"Node name","Service URL","Service token","ID (optional slug)","Poll interval (seconds)","Timeout (seconds)","Freshness TTL (seconds)"};
const char*const titles[]={"Overview","Guests","Storage","Sensors"};
void useTheme(bool light){lightTheme=light;const auto&p=light?LIGHT:DARK;BG=p.bg;CARD=p.card;INK=p.ink;MUTED=p.muted;CYAN=p.cyan;LAVENDER=p.lavender;MINT=p.mint;AMBER=p.amber;RED=p.red;EDGE=p.edge;}
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
bool stale(){return !snapshot->valid||ageSeconds()>HOMELAB_STALE_MS/1000.0;}
uint8_t failedSources(){uint8_t failed=0;for(int i=0;i<snapshot->nSources;i++){auto&s=snapshot->sources[i];if(s.enabled&&!s.ok)failed++;}return failed;}

bool demo(){return manualDemo||snapshot->demo;}
bool printerNode(){return !strcmp(snapshot->node.type,"klipper");}
bool printerFresh();
lv_obj_t *box(lv_obj_t *parent,int x,int y,int w,int h,uint32_t bg=CARD,int radius=7){auto*o=lv_obj_create(parent);lv_obj_set_pos(o,x,y);lv_obj_set_size(o,w,h);lv_obj_remove_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_SCROLLABLE|LV_OBJ_FLAG_CLICKABLE));lv_obj_set_style_bg_color(o,color(bg),0);lv_obj_set_style_bg_opa(o,LV_OPA_COVER,0);lv_obj_set_style_border_width(o,bg!=BG?1:0,0);lv_obj_set_style_border_color(o,color(EDGE),0);lv_obj_set_style_radius(o,radius,0);lv_obj_set_style_pad_all(o,0,0);return o;}
lv_obj_t *label(lv_obj_t *parent,const String &text,int x,int y,int w,uint32_t fg=INK,const lv_font_t *font=&lv_font_montserrat_12){
  auto*l=lv_label_create(parent);
  // The collector uses middle dots in NAS names; Montserrat supports bullets.
  // Allocate a normalized copy only for labels containing the missing glyph.
  if(strstr(text.c_str(),"\xC2\xB7")){String compatible=text;compatible.replace("\xC2\xB7","•");lv_label_set_text(l,compatible.c_str());}
  else lv_label_set_text(l,text.c_str());
  lv_obj_set_pos(l,x,y);lv_obj_set_width(l,w);lv_obj_set_height(l,lv_font_get_line_height(font));lv_label_set_long_mode(l,LV_LABEL_LONG_DOT);lv_obj_set_style_text_color(l,color(fg),0);lv_obj_set_style_text_font(l,font,0);return l;
}
void clickable(lv_obj_t *o,lv_event_cb_t cb,void *data=nullptr){lv_obj_add_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_CLICKABLE|LV_OBJ_FLAG_PRESS_LOCK));lv_obj_add_event_cb(o,tapGuard,LV_EVENT_ALL,nullptr);lv_obj_add_event_cb(o,cb,LV_EVENT_CLICKED,data);}
void render();
void finishField(bool cancel);
void openPage(uint8_t p){if(p==11&&page!=11&&page!=12)gpuListOrigin=page;if(p>=4&&p!=7&&p!=8&&page<4)detailOrigin=page;page=p;lastRotate=millis();render();}
void resetNodeView(){for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}historyCount=0;lastSequence=0;selectedGuestId=0;selectedDiskName[0]=0;selectedGpuId[0]=0;detailOrigin=0;gpuReturnPage=11;gpuListOrigin=5;page=0;strlcpy(trackedNodeId,snapshot->node.id,sizeof(trackedNodeId));}
void go(lv_event_t *e){uint8_t selected=uintptr_t(lv_event_get_user_data(e));openPage(printerNode()&&selected==6?3:selected);}
void chooseNode(lv_event_t*){pickerOrigin=page;openPage(13);}
void nodeGo(lv_event_t*e){size_t index=uintptr_t(lv_event_get_user_data(e));if(index<MAX_NODES&&nodeLinkIds[index][0]&&requestNodeSelection(nodeLinkIds[index])){manualDemo=false;readSnapshot(*snapshot,net);resetNodeView();render();}}

void guestGo(lv_event_t*e){int index=uintptr_t(lv_event_get_user_data(e));if(index<snapshot->nGuests){selectedGuestId=snapshot->guests[index].id;openPage(4);}}
void back(lv_event_t*){if(page==18){finishField(true);return;}openPage(page==14||page==15?7:page==16?15:page==17?16:page==13?pickerOrigin:page==4?1:page==9?2:page==11?gpuListOrigin:page==12?gpuReturnPage:detailOrigin);}
void diskGo(lv_event_t*e){int index=uintptr_t(lv_event_get_user_data(e));if(index<snapshot->nDisks){strlcpy(selectedDiskName,snapshot->disks[index].name,sizeof(selectedDiskName));openPage(9);}}
void gpuGo(lv_event_t*e){size_t index=uintptr_t(lv_event_get_user_data(e));if(index<MAX_GPUS&&gpuLinkIds[index][0]){strlcpy(selectedGpuId,gpuLinkIds[index],sizeof(selectedGpuId));gpuReturnPage=page==4?4:11;openPage(12);}}
void gpuIdentities(){for(size_t i=0;i<MAX_GPUS;i++)strlcpy(gpuLinkIds[i],i<snapshot->nGpus?snapshot->gpus[i].id:"",sizeof(gpuLinkIds[i]));}
void smallButton(lv_obj_t*parent,const String&t,int x,int y,int w,uint32_t fg,lv_event_cb_t cb,void*user=nullptr){auto*b=box(parent,x,y,w,40,CARD,7);lv_obj_set_style_border_color(b,color(fg),0);label(b,t,4,12,w-8,fg);clickable(b,cb,user);}
void progress(lv_obj_t *parent,int x,int y,int w,float v,uint32_t fg){auto*b=lv_bar_create(parent);lv_obj_remove_flag(b,LV_OBJ_FLAG_CLICKABLE);lv_obj_set_pos(b,x,y);lv_obj_set_size(b,w,4);lv_obj_set_style_bg_color(b,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(b,color(fg),LV_PART_INDICATOR);lv_obj_set_style_radius(b,3,LV_PART_MAIN);lv_obj_set_style_radius(b,3,LV_PART_INDICATOR);if(!isfinite(v))lv_obj_set_style_opa(b,LV_OPA_30,0);lv_bar_set_value(b,isfinite(v)?constrain(int(v),0,100):0,LV_ANIM_OFF);}
// Icons and history use bounded native draw tasks, with no bitmap buffers.
void drawIcon(lv_event_t*e){
  auto*o=lv_event_get_current_target_obj(e);auto*layer=lv_event_get_layer(e);lv_area_t a;lv_obj_get_coords(o,&a);int w=lv_area_get_width(&a),h=lv_area_get_height(&a);auto fg=lv_obj_get_style_text_color(o,0);
  auto X=[&](int v){return a.x1+v*(w-1)/24;};auto Y=[&](int v){return a.y1+v*(h-1)/24;};
  auto line=[&](int x1,int y1,int x2,int y2){lv_draw_line_dsc_t d;lv_draw_line_dsc_init(&d);d.color=fg;d.width=1;d.round_start=d.round_end=true;d.p1={X(x1),Y(y1)};d.p2={X(x2),Y(y2)};lv_draw_line(layer,&d);};
  auto rect=[&](int x,int y,int rw,int rh,int r){lv_draw_rect_dsc_t d;lv_draw_rect_dsc_init(&d);d.bg_opa=LV_OPA_TRANSP;d.border_color=fg;d.border_width=1;d.radius=r;lv_area_t b={X(x),Y(y),X(x+rw),Y(y+rh)};lv_draw_rect(layer,&d,&b);};
  auto arc=[&](int x,int y,int r,int start,int end){lv_draw_arc_dsc_t d;lv_draw_arc_dsc_init(&d);d.color=fg;d.width=1;d.center={X(x),Y(y)};d.radius=max(1,r*w/24);d.start_angle=start;d.end_angle=end;lv_draw_arc(layer,&d);};
  uint8_t kind=uintptr_t(lv_event_get_user_data(e));if(kind>=16){int tab=kind-16;kind=printerNode()?(tab==2?3:tab==3?6:tab):tab;}switch(kind){
    case 0:rect(3,3,7,7,1);rect(14,3,7,7,1);rect(3,14,7,7,1);rect(14,14,7,7,1);break;
    case 1:rect(3,3,18,7,1);rect(3,14,18,7,1);line(7,6,8,6);line(12,6,17,6);line(7,17,8,17);line(12,17,17,17);break;
    case 2:rect(4,3,16,18,2);arc(12,11,4,0,359);line(12,11,16,15);line(7,18,9,18);break;
    case 3:line(9,5,9,14);line(15,5,15,14);arc(12,5,3,180,359);arc(12,17,5,330,210);line(12,8,12,17);line(18,6,21,6);line(18,10,20,10);break;
    case 4:line(13,2,5,14);line(5,14,12,14);line(12,14,11,22);line(11,22,19,10);line(19,10,12,10);line(12,10,13,2);break;
    case 6:line(4,3,12,1);line(12,1,20,3);line(20,3,19,14);line(19,14,12,22);line(12,22,5,14);line(5,14,4,3);line(8,11,11,14);line(11,14,16,8);break;
    case 5:rect(5,5,14,14,1);rect(9,9,6,6,1);for(int i=7;i<=17;i+=5){line(i,2,i,5);line(i,19,i,22);line(2,i,5,i);line(19,i,22,i);}break;
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
  drawBrandMask(layer,glimdock::WORDMARK_DOT,a.x1+glimdock::WORDMARK_DOT_X,a.y1+glimdock::WORDMARK_DOT_Y,glimdock::PURPLE);
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
void sparkle(lv_obj_t*parent,const float *values,uint32_t fg){auto*c=box(parent,1,65,147,18,BG,0);lv_obj_set_style_bg_opa(c,LV_OPA_TRANSP,0);lv_obj_set_style_border_width(c,0,0);lv_obj_set_style_text_color(c,color(fg),0);lv_obj_add_event_cb(c,drawHistory,LV_EVENT_DRAW_MAIN,(void*)values);}
lv_obj_t *scroll(int y=6){auto*o=box(content,8,y,304,166-y,BG,0);lv_obj_add_flag(o,lv_obj_flag_t(LV_OBJ_FLAG_SCROLLABLE|LV_OBJ_FLAG_CLICKABLE|LV_OBJ_FLAG_PRESS_LOCK));lv_obj_set_scroll_dir(o,LV_DIR_VER);lv_obj_set_scrollbar_mode(o,LV_SCROLLBAR_MODE_AUTO);lv_obj_set_style_pad_bottom(o,8,0);lv_obj_set_style_bg_color(o,color(EDGE),LV_PART_SCROLLBAR);scrollArea=o;return o;}
void row(lv_obj_t*parent,int y,const String&key,const String&value,uint32_t fg=INK){auto*c=box(parent,0,y,296,38,CARD,9);label(c,key,10,12,166,MUTED);auto*l=label(c,value,156,11,130,fg);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);}
void section(lv_obj_t*parent,int y,const String&t,uint32_t fg=MUTED){label(parent,t,4,y,292,fg,&lv_font_montserrat_10);}
void heading(const String&title,const String&right="",bool withBack=false){
  int x=withBack?35:8;if(withBack){auto*b=box(content,0,0,40,40,BG,0);label(b,LV_SYMBOL_LEFT,10,8,18,CYAN);clickable(b,back);}label(content,title,x,7,right.length()?196:304-x,INK,&lv_font_montserrat_12);
  if(right.length()){auto*l=label(content,right,211,8,101,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);}
}
const lv_font_t*fitFont(const String&text,int width,const lv_font_t*preferred=&lv_font_montserrat_12){lv_point_t size;const lv_font_t*fonts[]={preferred,&lv_font_montserrat_10,&lv_font_montserrat_8};for(auto*font:fonts){lv_text_get_size(&size,text.c_str(),font,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);if(size.x<=width)return font;}return &lv_font_montserrat_8;}
void cell(lv_obj_t*p,int index,int top,const String&key,const String&value,uint32_t fg=INK){auto*c=box(p,(index%2)*152,top+(index/2)*45,147,40,CARD,6);label(c,key,7,6,133,MUTED,&lv_font_montserrat_10);label(c,value,7,21,133,fg,fitFont(value,133));}
int note(lv_obj_t*p,int y,const String&text,uint32_t fg=MUTED){lv_point_t size;lv_text_get_size(&size,text.c_str(),&lv_font_montserrat_10,0,0,282,LV_TEXT_FLAG_NONE);auto*l=label(p,text,7,y,282,fg,&lv_font_montserrat_10);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);return y+size.y+8;}
double sampleAge(double age,double updated){double elapsed=(millis()-snapshot->received)/1000.0;if(isfinite(age))return max(0.0,age+elapsed);time_t now=time(nullptr);return isfinite(updated)&&now>1700000000?max(0.0,double(now)-updated):NAN;}
bool guestOs(const Guest&g){return !strcmp(g.memoryBasis,"guest-os")||!strcmp(g.memoryBasis,"guest-os-arc");}
bool guestArc(const Guest&g){return !strcmp(g.memoryBasis,"guest-os-arc");}
String guestAge(const Guest&g){double age=sampleAge(g.memAge,g.memUpdated);return isfinite(age)?number(age,0,"s ago"):String("age unknown");}
double guestPrimary(const Guest&g){return guestArc(g)?g.noncache:g.used;}
void metricValue(lv_obj_t*p,double value,int digits,uint32_t fg){String text=number(value,digits);label(p,text,8,22,128,fg,&lv_font_montserrat_32);lv_point_t size;lv_text_get_size(&size,text.c_str(),&lv_font_montserrat_32,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);label(p,"%",min(130,8+size.x+3),37,18,fg,&lv_font_montserrat_14);}
void header(){
  bool headerStale=stale()||(printerNode()&&!printerFresh());
  lv_obj_set_style_bg_color(lv_screen_active(),color(BG),0);lv_obj_set_style_bg_color(content,color(BG),0);
  lv_obj_set_style_text_color(headerTitle,color(lightTheme?glimdock::INK_LIGHT:glimdock::INK_DARK),0);lv_obj_set_style_bg_color(headerMark,color(glimdock::PURPLE),0);lv_obj_set_style_text_color(headerHost,color(MUTED),0);
  lv_obj_set_style_bg_color(headerLine,color(EDGE),0);
  lv_obj_set_style_bg_color(nodeButton,color(BG),0);lv_obj_set_style_text_color(lv_obj_get_child(nodeButton,1),color(CYAN),0);
  lv_obj_set_style_bg_color(settingsButton,color(BG),0);lv_obj_set_style_text_color(lv_obj_get_child(settingsButton,0),color(MUTED),0);
  String host=snapshot->node.name[0]?String(snapshot->node.name):snapshot->valid?String(snapshot->host):String("Choose node");lv_label_set_text(headerHost,host.c_str());
  uint32_t fg=demo()?LAVENDER:!net.wifi?RED:net.loading||headerStale?AMBER:MINT;lv_obj_set_style_text_color(headerState,color(fg),0);
  uint32_t pill=demo()||headerStale?(lightTheme?0xfff0d8:0x392b1d):net.wifi?(lightTheme?0xe0f3e9:0x17362e):(lightTheme?0xfde7e9:0x3b202e);lv_obj_set_style_bg_color(statePill,color(pill),0);lv_obj_set_style_border_width(statePill,0,0);
  lv_label_set_text(headerState,demo()?"DEMO":net.loading?(net.wifi?"LOADING":"OFFLINE"):snapshot->valid?(printerNode()&&!strcmp(snapshot->node.status,"offline")?"OFFLINE":headerStale?"STALE":"LIVE"):net.setup?"SETUP":"OFFLINE");
  lv_obj_set_style_bg_color(tabBar,color(CARD),0);lv_obj_set_style_border_color(tabBar,color(EDGE),0);
  int active=page<4?page:page==4?1:page==9?2:page==12&&gpuReturnPage==4?1:page==5||page==6||page==10||page==11||page==12?detailOrigin:-1;
  const char*const printerTitles[]={"Overview","Job","Temps","Health"};
  for(int i=0;i<4;i++){lv_label_set_text(lv_obj_get_child(tabs[i],1),printerNode()?printerTitles[i]:titles[i]);lv_obj_set_style_bg_color(tabs[i],color(i==active?(lightTheme?CARD:0x1a3d49):CARD),0);for(int j=0;j<2;j++)lv_obj_set_style_text_color(lv_obj_get_child(tabs[i],j),color(i==active?CYAN:MUTED),0);auto*rule=lv_obj_get_child(tabs[i],2);lv_obj_set_style_bg_color(rule,color(CYAN),0);if(i==active)lv_obj_remove_flag(rule,LV_OBJ_FLAG_HIDDEN);else lv_obj_add_flag(rule,LV_OBJ_FLAG_HIDDEN);}
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
  auto&p=snapshot->printer;bool fresh=printerFresh();uint32_t fg=printerStateColor();auto*r=box(content,8,6,304,18,BG,0);auto*dot=box(r,1,7,4,4,fg,2);lv_obj_set_style_border_width(dot,0,0);label(r,(fresh||!strcmp(p.klippyState,"startup")||!strcmp(p.klippyState,"shutdown")||!strcmp(p.klippyState,"disconnected")||!strcmp(snapshot->node.status,"offline")?printerState():String("Stale printer data"))+(snapshot->nAlerts?" • "+String(snapshot->nAlerts)+" alerts":""),10,3,230,fg,&lv_font_montserrat_10);auto*age=label(r,number(printerAge(),0,"s ago  >"),238,3,66,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(age,LV_TEXT_ALIGN_RIGHT,0);clickable(r,go,(void*)3);
  auto*hero=box(content,8,29,304,64,CARD,9);label(hero,printerState(),10,8,190,INK,&lv_font_montserrat_14);String progressText=number(fresh?p.progress:NAN,1,"%");lv_point_t progressSize;lv_text_get_size(&progressSize,progressText.c_str(),&lv_font_montserrat_28,0,0,LV_COORD_MAX,LV_TEXT_FLAG_NONE);auto*v=label(hero,progressText,207,8,86,CYAN,progressSize.x<=86?&lv_font_montserrat_28:&lv_font_montserrat_18);lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);progress(hero,10,38,284,fresh?p.progress:NAN,CYAN);label(hero,p.filename[0]?String(p.filename):String("No active file"),10,51,284,MUTED,&lv_font_montserrat_8);clickable(hero,go,(void*)1);
  auto*estimate=box(content,8,97,304,17,BG,0);label(estimate,"Est. remaining",0,3,142,MUTED,&lv_font_montserrat_10);v=label(estimate,printRemaining(),170,1,134,INK,&lv_font_montserrat_12);lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);clickable(estimate,go,(void*)1);
  for(int i=0;i<2;i++){const Heater*h=primaryHeater(i==1);auto*c=box(content,8+i*155,117,149,39,CARD,7);label(c,i?"Bed":"Nozzle",8,5,131,MUTED,&lv_font_montserrat_8);label(c,number(fresh&&h?h->temp:NAN,1),8,17,67,INK,&lv_font_montserrat_14);label(c,"/ "+number(fresh&&h?h->target:NAN,0)+" °C",69,21,72,MUTED,&lv_font_montserrat_10);clickable(c,go,(void*)2);}

}
void printerJob(){
  auto&p=snapshot->printer;bool fresh=printerFresh();heading("Current job",printerState());auto*s=scroll(28);int y=note(s,0,p.filename[0]?String(p.filename):String("No active file"),INK);cell(s,0,y,"Progress",number(fresh?p.progress:NAN,1,"%"),CYAN);cell(s,1,y,"Layer / total",number(fresh?p.currentLayer:NAN,0)+" / "+number(fresh?p.totalLayers:NAN,0));cell(s,2,y,"Printing time",span(fresh?p.printDuration:NAN));cell(s,3,y,"Total job time",span(fresh?p.totalDuration:NAN));cell(s,4,y,"Est. remaining",printRemaining(),LAVENDER);cell(s,5,y,"Est. finish (UTC)",printEta(),LAVENDER);cell(s,6,y,"Filament used",number(fresh?p.filament:NAN,0," mm"));cell(s,7,y,"Slicer estimate",span(p.slicerTime));cell(s,8,y,"Cooling fan",number(fresh?p.fan:NAN,0,"%"));cell(s,9,y,"Filament sensor",!fresh||p.filamentDetected<0?String("--"):p.filamentDetected?String("Detected"):String("Not detected"));y+=233; y=note(s,y,"Progress source: "+String(p.progressBasis[0]?p.progressBasis:"unavailable"));y=note(s,y,"Remaining estimate: "+String(p.etaBasis[0]?p.etaBasis:"unavailable"));if(p.message[0])y=note(s,y,p.message);if(p.error[0])note(s,y,p.error,RED);
}
String temperatureName(const char*name){if(!strcmp(name,"mcu_temp"))return "MCU temperature";if(!strcmp(name,"raspberry_pi"))return "Raspberry Pi";String result=name;result.replace("temperature_sensor ","");result.replace("temperature_host ","");result.replace("_"," ");return result;}
void printerTemps(){
  auto&p=snapshot->printer;bool fresh=printerFresh();heading("Temperatures",String(p.nHeaters+p.nTemps)+" readings");auto*s=scroll(28);int y=0;if(!p.nHeaters&&!p.nTemps)y=note(s,y,"Printer temperature readings are unavailable.",AMBER);
  for(int i=0;i<p.nHeaters;i++){auto&h=p.heaters[i];auto*c=box(s,0,y,296,58,CARD,7);label(c,h.name,8,8,171,INK,&lv_font_montserrat_12);auto*v=label(c,number(fresh?h.temp:NAN,1," °C"),180,7,108,AMBER,&lv_font_montserrat_18);lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);label(c,"Target "+number(fresh?h.target:NAN,0," °C")+" • Duty "+number(fresh?h.duty:NAN,1,"%"),8,34,280,MUTED,&lv_font_montserrat_10);y+=64;}
  for(int i=0;i<p.nTemps;i++){auto*c=box(s,0,y,296,38,CARD,7);label(c,temperatureName(p.temperatures[i].name),10,12,168,MUTED);String value=number(fresh?p.temperatures[i].temp:NAN,1," °C");auto*v=label(c,value,182,11,104,AMBER,fitFont(value,104));lv_obj_set_style_text_align(v,LV_TEXT_ALIGN_RIGHT,0);y+=44;}
  row(s,y,"Cooling fan",number(fresh?p.fan:NAN,0,"%"));y+=46;note(s,y,"Heater duty is commanded PWM %, not measured watts.");
}
void printerHealth(){
  auto&p=snapshot->printer;heading("Printer health",snapshot->node.status);auto*s=scroll(28);int y=0;row(s,y,"Klipper",p.klippyState[0]?String(p.klippyState):String("Unavailable"),printerStateColor());y+=44;row(s,y,"Job state",printerState(),printerStateColor());y+=44;row(s,y,"Sample age",number(printerAge(),0,"s"),printerFresh()?MINT:AMBER);y+=44;if(p.message[0])y=note(s,y,p.message);if(p.error[0])y=note(s,y,p.error,RED);
  if(!snapshot->nAlerts){row(s,y,"Active alerts","None",MINT);y+=44;}for(int i=0;i<snapshot->nAlerts;i++){auto&a=snapshot->alerts[i];section(s,y,a.severity,!strcmp(a.severity,"critical")?RED:AMBER);y=note(s,y+18,a.message,INK);}
  y=note(s,y,"Node: "+String(snapshot->node.name));y=note(s,y,"Address: "+String(snapshot->node.address));if(p.hostName[0])y=note(s,y,"Host: "+String(p.hostName));section(s,y,"PRINTER SOURCES");y+=20;for(int i=0;i<snapshot->nSources;i++){auto&source=snapshot->sources[i];row(s,y,source.name,!source.enabled?String("Disabled"):source.ok?String("OK • ")+number(sampleAge(source.age,source.updated),0,"s"):String("Unavailable"),!source.enabled?MUTED:source.ok?MINT:AMBER);y+=44;if(source.error[0])y=note(s,y,source.error,AMBER);}if(!snapshot->nSources)note(s,y,"Printer source status unavailable.",AMBER);
}
void nodePicker(){
  heading("Choose node",String(snapshot->nNodes)+" nodes",true);auto*s=scroll(28);int y=0;for(int i=0;i<MAX_NODES;i++)strlcpy(nodeLinkIds[i],i<snapshot->nNodes?snapshot->nodes[i].id:"",sizeof(nodeLinkIds[i]));
  for(int i=0;i<snapshot->nNodes;i++){auto&n=snapshot->nodes[i];auto*c=box(s,0,y,296,62,CARD,7);if(!strcmp(n.id,snapshot->node.id))lv_obj_set_style_border_color(c,color(CYAN),0);label(c,n.name,10,8,197,INK,&lv_font_montserrat_12);auto*state=label(c,n.status,211,10,75,!strcmp(n.status,"healthy")?MINT:AMBER,&lv_font_montserrat_8);lv_obj_set_style_text_align(state,LV_TEXT_ALIGN_RIGHT,0);int height=note(c,30,String(!strcmp(n.type,"klipper")?"Klipper":"Proxmox")+" • "+n.address);lv_obj_set_height(c,max(62,height+4));y+=max(62,height+4)+6;clickable(c,nodeGo,(void*)(uintptr_t)i);}
  if(!snapshot->nNodes)note(s,0,"Connect to a collector feed to discover configured nodes.",MUTED);
}
void overview(){
  uint8_t issues=snapshot->nAlerts;bool incomplete=failedSources()>0||!isfinite(snapshot->cpu)||!isfinite(snapshot->memTotal);bool critical=false;for(int i=0;i<snapshot->nAlerts;i++)if(!strcmp(snapshot->alerts[i].severity,"critical"))critical=true;uint32_t health=stale()?AMBER:issues?(critical?RED:AMBER):incomplete?AMBER:MINT;
  auto*r=box(content,8,6,304,18,BG,0);String healthText=stale()?"Feed disconnected":issues?String(issues)+" alert"+(issues==1?"":"s")+" need attention":incomplete?"Some metrics unavailable":"No active alerts";
  auto*dot=box(r,1,7,4,4,health,2);lv_obj_set_style_border_width(dot,0,0);label(r,healthText,10,3,232,health,&lv_font_montserrat_10);auto*a=label(r,stale()?"STALE  >":number(ageSeconds(),0,"s ago  >"),242,3,62,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(a,LV_TEXT_ALIGN_RIGHT,0);clickable(r,go,(void*)6);
  auto*cpu=box(content,8,29,149,84,CARD,9);label(cpu,"CPU",8,7,56,MUTED,&lv_font_montserrat_10);metricValue(cpu,snapshot->cpu,1,CYAN);label(cpu,(snapshot->nCores?String(snapshot->nCores):String("--"))+" threads • load "+number(snapshot->load[0],1),8,55,135,MUTED,&lv_font_montserrat_10);sparkle(cpu,cpuHistory,CYAN);clickable(cpu,go,(void*)5);
  auto*ram=box(content,163,29,149,84,CARD,9);label(ram,"MEMORY",8,7,100,MUTED,&lv_font_montserrat_10);metricValue(ram,memoryPct(snapshot->memUsed,snapshot->memTotal),0,LAVENDER);label(ram,number(snapshot->memUsed/1073741824,1)+" / "+number(snapshot->memTotal/1073741824,1)+" GiB",8,55,135,MUTED,&lv_font_montserrat_10);sparkle(ram,ramHistory,LAVENDER);clickable(ram,go,(void*)10);
  auto*t=box(content,8,118,149,31,CARD,7);icon(t,3,7,8,13,AMBER);label(t,"CPU",25,10,34,MUTED,&lv_font_montserrat_10);auto*tv=label(t,number(snapshot->temp,0," °C"),70,8,72,INK,&lv_font_montserrat_14);lv_obj_set_style_text_align(tv,LV_TEXT_ALIGN_RIGHT,0);clickable(t,go,(void*)3);
  auto*p=box(content,163,118,149,31,CARD,7);icon(p,4,7,8,13,AMBER);label(p,"Package",25,10,50,MUTED,&lv_font_montserrat_10);auto*pv=label(p,number(snapshot->watts,snapshot->watts>=100?0:1," W"),74,8,68,MINT,&lv_font_montserrat_14);lv_obj_set_style_text_align(pv,LV_TEXT_ALIGN_RIGHT,0);clickable(p,go,(void*)5);
  String footer;uint32_t phase=(millis()/6000)%(snapshot->nGpus?4:3);
  if(phase==0){int online=0;for(int i=0;i<snapshot->nGuests;i++)if(!strcmp(snapshot->guests[i].status,"running"))online++;label(content,String(LV_SYMBOL_DOWN)+" "+rate(snapshot->rx)+"  "+String(LV_SYMBOL_UP)+" "+rate(snapshot->tx),8,155,174,MUTED,&lv_font_montserrat_10);auto*l=label(content,String(online)+"/"+snapshot->nGuests+" guests • "+uptime(snapshot->uptime),180,155,132,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);return;}
  else if(phase==1){int online=0;for(int i=0;i<snapshot->nGuests;i++)if(!strcmp(snapshot->guests[i].status,"running"))online++;footer="GUESTS "+String(online)+"/"+snapshot->nGuests+"   DISKS "+snapshot->nDisks+"   FAULTS "+number(snapshot->faultEvents24h,0)+"/24h";}
  else if(phase==3){int selected=0;for(int i=0;i<snapshot->nGpus;i++)if(!strcmp(snapshot->gpus[i].kind,"discrete")&&isfinite(snapshot->gpus[i].utilization)){selected=i;break;}auto&g=snapshot->gpus[selected];footer=String(snapshot->nGpus)+" GPUs • "+(!strcmp(g.utilizationKind,"busiest-engine")?String("ENGINE "):String("GPU "))+number(g.utilization,0,"%")+" • "+number(g.temp,0," °C")+" • "+number(g.power,0," W")+"  >";auto*b=box(content,8,151,304,15,BG,0);label(b,footer,0,3,304,CYAN,fitFont(footer,304,&lv_font_montserrat_10));clickable(b,go,(void*)11);return;}
  else{float hottest=NAN,fan=NAN;for(int i=0;i<snapshot->nDisks;i++)if(isfinite(snapshot->disks[i].temp))hottest=isfinite(hottest)?max(hottest,snapshot->disks[i].temp):snapshot->disks[i].temp;for(int i=0;i<snapshot->nSensors;i++){auto&v=snapshot->sensors[i];if(!strcmp(v.kind,"fan")&&isfinite(v.value))fan=isfinite(fan)?max(fan,v.value):v.value;}footer="DISK MAX "+number(hottest,0," C")+"   FAN MAX "+number(fan,0," RPM");}
  label(content,footer,8,155,304,MUTED,&lv_font_montserrat_10);
}
void guests(){
  int running=0;for(int i=0;i<snapshot->nGuests;i++)if(!strcmp(snapshot->guests[i].status,"running"))running++;
  heading("Virtual machines & containers",String(running)+" running");auto*s=scroll(28);
  if(!snapshot->nGuests){row(s,0,"Guest inventory","Unavailable",AMBER);return;}
  for(int i=0;i<snapshot->nGuests;i++){auto&g=snapshot->guests[i];double primary=guestPrimary(g);auto*c=box(s,0,i*68,296,63);auto*tag=box(c,7,7,25,14,lightTheme?0xe8eef4:0x213347,3);lv_obj_set_style_border_width(tag,0,0);label(tag,!strcmp(g.type,"lxc")?"CT":"VM",4,3,22,MUTED,&lv_font_montserrat_8);label(c,g.name,38,7,183,INK,&lv_font_montserrat_12);auto*status=label(c,g.status,224,9,65,!strcmp(g.status,"running")?MINT:MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(status,LV_TEXT_ALIGN_RIGHT,0);
    label(c,String(g.id)+" • CPU "+number(g.cpu,1,"%"),7,28,128,MUTED,&lv_font_montserrat_10);String ram=String(guestArc(g)?"~":"")+number(primary/1073741824,1)+" / "+number(g.total/1073741824,1)+" GiB  >";auto*mem=label(c,ram,135,28,154,LAVENDER,fitFont(ram,154,&lv_font_montserrat_10));lv_obj_set_style_text_align(mem,LV_TEXT_ALIGN_RIGHT,0);
    String basis=guestArc(g)?"OS/apps ~ • ARC "+bytes(g.cache)+" • "+guestAge(g):guestOs(g)?"Guest RAM • "+guestAge(g):"PVE RAM • Proxmox accounting";if(!isfinite(primary))basis=(guestArc(g)?String("OS/apps ~ unavailable"):guestOs(g)?String("Guest RAM unavailable"):String("PVE RAM unavailable"))+" • "+guestAge(g);label(c,basis,7,43,282,isfinite(primary)?MUTED:AMBER,&lv_font_montserrat_8);progress(c,7,56,282,memoryPct(primary,g.total),strcmp(g.status,"running")?MUTED:LAVENDER);clickable(c,guestGo,(void*)(uintptr_t)i);
  }
}
void guestDetail(){
  int selectedGuest=-1;for(int i=0;i<snapshot->nGuests;i++)if(snapshot->guests[i].id==selectedGuestId)selectedGuest=i;
  if(selectedGuest<0){label(content,"Guest no longer present",12,18,296,AMBER);return;}
  auto&g=snapshot->guests[selectedGuest];heading(g.name,String(!strcmp(g.type,"lxc")?"CT ":"VM ")+g.id,true);label(content,g.status,8,29,104,strcmp(g.status,"running")?MUTED:MINT,&lv_font_montserrat_10);auto*up=label(content,"Uptime "+uptime(g.uptime),114,29,198,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(up,LV_TEXT_ALIGN_RIGHT,0);
  auto*s=scroll(46);cell(s,0,0,"CPU",number(g.cpu,1,"%"),CYAN);cell(s,1,0,guestArc(g)?"Used incl. ARC":guestOs(g)?"Guest RAM":"PVE RAM",number(g.used/1073741824,1)+" / "+number(g.total/1073741824,1)+" GiB",LAVENDER);cell(s,2,0,"Network in",rate(g.rx));cell(s,3,0,"Network out",rate(g.tx));cell(s,4,0,"Disk read",rate(g.read));cell(s,5,0,"Disk write",rate(g.write));int y=139;
  y=note(s,y,String(guestArc(g)?"Guest OS + ARC":guestOs(g)?"Guest OS":"Proxmox accounting")+" • "+guestAge(g),isfinite(g.used)?MUTED:AMBER);if(g.memError[0])y=note(s,y,g.memError,AMBER);
  int i=0;cell(s,i++,y,"Available",bytes(g.available));if(guestArc(g)){cell(s,i++,y,"ARC cache",bytes(g.cache),LAVENDER);cell(s,i++,y,"OS/apps ~ estimate",bytes(g.noncache),LAVENDER);}cell(s,i++,y,"Assigned RAM",bytes(g.assigned));cell(s,i++,y,"Host footprint",bytes(g.hostMem));cell(s,i++,y,"Proxmox RAM",bytes(g.pveUsed)+" / "+bytes(g.pveTotal));y+=((i+1)/2)*45+8;
  if(guestArc(g))y=note(s,y,"OS/apps ~ = guest used minus ARC; this is an estimate. ARC cache can be reclaimed.");
  gpuIdentities();String owner="vm:"+String(g.id);bool found=false;for(int k=0;k<snapshot->nGpus;k++)if(owner==snapshot->gpus[k].owner){if(!found){section(s,y,"ASSIGNED GPUS");y+=22;found=true;}smallButton(s,snapshot->gpus[k].name,0,y,296,CYAN,gpuGo,(void*)(uintptr_t)k);y+=46;}
  note(s,y,"Disk values are throughput, not guest capacity.");
}
void storage(){
  heading("Storage","Pools & disks");auto*s=scroll(28);int y=0;
  if(!snapshot->nPools){row(s,y,"Storage pools","Unavailable",AMBER);y+=44;}
  for(int i=0;i<snapshot->nPools;i++){auto&p=snapshot->pools[i];auto*c=box(s,0,y,296,52);label(c,p.name,7,7,202,INK,&lv_font_montserrat_12);auto*l=label(c,number(memoryPct(p.used,p.total),0,"%"),211,7,78,INK);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,p.status,7,28,104,MUTED,&lv_font_montserrat_10);l=label(c,bytes(p.used)+" / "+bytes(p.total),111,28,178,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);progress(c,7,44,282,memoryPct(p.used,p.total),memoryPct(p.used,p.total)>85?AMBER:CYAN);y+=57;}
  section(s,y+3,"PHYSICAL DISKS • SMART");y+=23;
  if(!snapshot->nDisks){row(s,y,"Disk telemetry","Unavailable",AMBER);y+=44;}
  for(int i=0;i<snapshot->nDisks;i++){auto&d=snapshot->disks[i];auto*c=box(s,0,y,296,55);label(c,d.name,7,7,197,INK,&lv_font_montserrat_12);auto*l=label(c,number(d.temp,0," °C"),208,7,81,INK);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,d.model,7,28,180,MUTED,&lv_font_montserrat_8);bool ok=!strcmp(d.health,"passed");l=label(c,String(d.zfsStatus[0]?d.zfsStatus:d.health)+"  >",183,28,106,ok||!strcmp(d.zfsStatus,"ONLINE")?MINT:!strcmp(d.health,"failed")?RED:MUTED,&lv_font_montserrat_8);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);label(c,"SMART "+String(d.health),7,42,280,MUTED,&lv_font_montserrat_8);clickable(c,diskGo,(void*)(uintptr_t)i);y+=60;}
}
void diskDetail(){
  int index=-1;for(int i=0;i<snapshot->nDisks;i++)if(!strcmp(snapshot->disks[i].name,selectedDiskName))index=i;
  if(index<0){label(content,"Disk no longer present",12,18,296,AMBER);return;}
  auto&d=snapshot->disks[index];heading(d.name,d.status,true);label(content,d.model,8,28,304,MUTED,&lv_font_montserrat_10);auto*s=scroll(46);int i=0;
  cell(s,i++,0,"SMART health",d.health,!strcmp(d.health,"passed")?MINT:!strcmp(d.health,"failed")?RED:MUTED);cell(s,i++,0,"Temperature",number(d.temp,1," °C"),AMBER);cell(s,i++,0,"Read",rate(d.read));cell(s,i++,0,"Write",rate(d.write));
  if(d.zfsStatus[0]){cell(s,i++,0,"ZFS status",d.zfsStatus,!strcmp(d.zfsStatus,"ONLINE")?MINT:AMBER);cell(s,i++,0,"Checksum errors",number(d.zfsChecksumErrors,0));cell(s,i++,0,"ZFS read errors",number(d.zfsReadErrors,0));cell(s,i++,0,"ZFS write errors",number(d.zfsWriteErrors,0));}
  int y=((i+1)/2)*45+5;section(s,y,"DETAILED SMART");y+=22;i=0;cell(s,i++,y,"Life used",number(d.wear,0,"%"));cell(s,i++,y,"Spare capacity",number(d.spare,0,"%"));cell(s,i++,y,"Media errors",number(d.mediaErrors,0));cell(s,i++,y,"Power-on hours",number(d.powerHours,0,"h"));cell(s,i++,y,"Reallocated sectors",number(d.reallocated,0));cell(s,i++,y,"Pending sectors",number(d.pending,0));section(s,y+140,"Unavailable SMART fields are shown as --.");
}
int sensorFilter=1;const char*const sensorKinds[]={"all","temperature","fan","voltage","power","current"};
void filter(lv_event_t*e){sensorFilter=uintptr_t(lv_event_get_user_data(e));render();}
void sensors(){
  heading("Hardware sensors",String(snapshot->nSensors)+" readings");
  // Six category controls retain 40px targets while matching the compact chips.
  auto*chips=box(content,8,22,252,40,BG,0);lv_obj_add_flag(chips,LV_OBJ_FLAG_SCROLLABLE);lv_obj_set_scroll_dir(chips,LV_DIR_HOR);lv_obj_set_scrollbar_mode(chips,LV_SCROLLBAR_MODE_OFF);const char*names[]={"All","Temp","Fans","Volts","Power","Amps"};
  for(int i=0;i<6;i++){auto*c=box(chips,i*42,0,40,40,sensorFilter==i?(lightTheme?CARD:0x1a3d49):CARD,5);if(sensorFilter==i)lv_obj_set_style_border_color(c,color(CYAN),0);label(c,names[i],3,14,34,sensorFilter==i?CYAN:MUTED,&lv_font_montserrat_10);clickable(c,filter,(void*)(uintptr_t)i);}auto*power=box(content,266,22,46,40,CARD,5);icon(power,4,16,12,14,CYAN);clickable(power,go,(void*)5);
  auto*s=scroll(66);int y=0,shown=0;String lastChip;
  for(int i=0;i<snapshot->nSensors;i++){auto&v=snapshot->sensors[i];if(sensorFilter&&strcmp(v.kind,sensorKinds[sensorFilter]))continue;shown++;
    if(lastChip!=v.chip){section(s,y,v.chip);lastChip=v.chip;y+=18;}
    uint32_t fg=INK;bool critical=isfinite(v.crit)&&isfinite(v.value)&&v.value>=v.crit;bool hot=isfinite(v.high)&&isfinite(v.value)&&v.value>=v.high;if(critical)fg=RED;else if(hot)fg=AMBER;
    String reading=number(v.value,!strcmp(v.kind,"voltage")||!strcmp(v.kind,"current")?2:!strcmp(v.kind,"fan")?0:1," ")+v.unit;auto*c=box(s,0,y,296,43);label(c,v.name,7,6,168,INK,&lv_font_montserrat_12);auto*l=label(c,reading,180,6,109,fg,fitFont(reading,109));lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);
    String thresholds=String(v.kind)+" • High "+number(v.high)+" / Crit "+number(v.crit);label(c,thresholds,7,27,282,MUTED,fitFont(thresholds,282,&lv_font_montserrat_10));y+=48;
  }
  if(!shown){
    const Source*source=nullptr;for(int i=0;i<snapshot->nSources;i++)if(!strcmp(snapshot->sources[i].name,"sensors")){source=&snapshot->sources[i];break;}
    bool healthy=source&&source->enabled&&source->ok;String title,message;
    if(healthy){title=sensorFilter==5?"No current sensors reported":sensorFilter==4?"No power sensors reported":sensorFilter?"No readings in this category":"No sensor readings reported";message=sensorFilter==5?"The reported hardware/driver readings do not expose amperage.":"The sensor source is healthy; this category has no readings.";}
    else{title=source&&!source->enabled?"Sensor source disabled":source?"Hardware sensors unavailable":"Sensor status unavailable";message="Open Health for sensor-source status and collection errors.";}
    auto*c=box(s,0,0,296,80);label(c,title,7,9,282,healthy?INK:AMBER,&lv_font_montserrat_12);y=note(c,32,message)+4;lv_obj_set_height(c,y);y+=8;
  }
  if(snapshot->truncated){section(s,y,"Inventory exceeds display limits; see collector for the rest.",AMBER);}
}
void powerDetail(){
  heading("CPU & power","",true);auto*s=scroll(28);cell(s,0,0,"CPU usage",number(snapshot->cpu,1,"%"),CYAN);cell(s,1,0,"Package power",number(snapshot->watts,1," W"),MINT);cell(s,2,0,"Average frequency",number(snapshot->mhz,0," MHz"));cell(s,3,0,"Busy frequency",number(snapshot->busyMhz,0," MHz"));cell(s,4,0,"CPU temperature",number(snapshot->temp,0," °C"),AMBER);cell(s,5,0,"I/O wait",number(snapshot->iowait,1,"%"));smallButton(s,"GPUs • "+String(snapshot->nGpus)+" devices  >",0,140,296,CYAN,go,(void*)11);int y=190;
  section(s,y,"LOGICAL CPU USAGE / %");y+=22;
  for(int i=0;i<snapshot->nCores;i++){float v=snapshot->cores[i];int heat=isfinite(v)?int(20+constrain(v,0.0f,100.0f)*.65):0;auto*c=box(s,(i%8)*37,y+(i/8)*32,34,28,CARD,4);lv_obj_set_style_bg_color(c,lv_color_mix(color(CYAN),color(CARD),heat),0);lv_obj_set_style_border_width(c,0,0);auto*index=label(c,String(i),0,2,34,INK,&lv_font_montserrat_8);lv_obj_set_style_text_align(index,LV_TEXT_ALIGN_CENTER,0);auto*value=label(c,number(v,0),0,14,34,INK,&lv_font_montserrat_10);lv_obj_set_style_text_align(value,LV_TEXT_ALIGN_CENTER,0);}
  if(!snapshot->nCores){row(s,y,"Core readings","Unavailable",AMBER);y+=44;}else y+=((snapshot->nCores+7)/8)*32+8;
  section(s,y,"IDLE RESIDENCY");y+=22;if(!snapshot->nCstates){row(s,y,"C-state readings","Unavailable",AMBER);y+=44;}else{for(int i=0;i<snapshot->nCstates;i++)cell(s,i,y,snapshot->cstateNames[i],number(snapshot->cstates[i],1,"%"));y+=((snapshot->nCstates+1)/2)*45+8;}
  smallButton(s,"Memory, load & host I/O",0,y,296,CYAN,go,(void*)10);
}
void memoryDetail(){
  heading("Memory","",true);auto*s=scroll(28);cell(s,0,0,"Used",bytes(snapshot->memUsed),LAVENDER);cell(s,1,0,"Total",bytes(snapshot->memTotal));cell(s,2,0,"Swap used",bytes(snapshot->swapUsed));cell(s,3,0,"Swap total",bytes(snapshot->swapTotal));cell(s,4,0,"ZFS ARC",bytes(snapshot->arc));cell(s,5,0,"Load 1 / 5 / 15",number(snapshot->load[0])+" / "+number(snapshot->load[1])+" / "+number(snapshot->load[2]));
  auto*l=label(s,"Used = total - available.\nGuest usage can include VM overhead.",7,140,282,MUTED,&lv_font_montserrat_10);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);
  section(s,179,"HOST / I/O");cell(s,0,201,"Host uptime",uptime(snapshot->uptime));cell(s,1,201,"Host address",snapshot->ip);cell(s,2,201,"Network in",rate(snapshot->rx));cell(s,3,201,"Network out",rate(snapshot->tx));cell(s,4,201,"Disk read",rate(snapshot->read));cell(s,5,201,"Disk write",rate(snapshot->write));
}
bool engineLoad(const Gpu&g){return !strcmp(g.utilizationKind,"busiest-engine");}
uint32_t gpuStateColor(const Gpu&g){return !strcmp(g.status,"active")?MINT:!strcmp(g.status,"inventory")?MUTED:AMBER;}
String gpuOwner(const Gpu&g){if(!strcmp(g.owner,"host"))return "Host";if(!strncmp(g.owner,"vm:",3)){int id=atoi(g.owner+3);for(int i=0;i<snapshot->nGuests;i++)if(snapshot->guests[i].id==id)return String(g.owner)+" • "+snapshot->guests[i].name;}return g.owner[0]?String(g.owner):String("Unknown owner");}
String gpuMemory(const Gpu&g){return isfinite(g.memTotal)?bytes(g.memUsed)+" / "+bytes(g.memTotal):bytes(g.memUsed);}
void gpuList(){
  heading("GPUs",String(snapshot->nGpus)+" devices",true);auto*s=scroll(28);gpuIdentities();
  if(!snapshot->nGpus){row(s,0,"GPU inventory","Unavailable",AMBER);note(s,48,"The collector has not supplied GPU inventory.");return;}
  for(int i=0;i<snapshot->nGpus;i++){auto&g=snapshot->gpus[i];auto*c=box(s,0,i*79,296,74);icon(c,5,7,7,14,CYAN);label(c,g.name,27,7,193,INK,&lv_font_montserrat_12);auto*state=label(c,g.status,219,9,70,gpuStateColor(g),&lv_font_montserrat_8);lv_obj_set_style_text_align(state,LV_TEXT_ALIGN_RIGHT,0);label(c,gpuOwner(g)+" • "+g.kind,7,28,282,MUTED,&lv_font_montserrat_10);
    String metrics=String(engineLoad(g)?"Engine ":"GPU ")+number(g.utilization,0,"%")+" • "+number(g.temp,0," °C")+" • "+gpuMemory(g);label(c,metrics,7,45,282,INK,fitFont(metrics,282,&lv_font_montserrat_10));double age=sampleAge(g.age,g.updated);label(c,String(!strcmp(g.status,"inventory")?"Inventory only • ":g.error[0]?"Limited telemetry • ":"Updated • ")+number(age,0,"s ago")+"  >",7,60,282,g.error[0]?AMBER:MUTED,&lv_font_montserrat_8);clickable(c,gpuGo,(void*)(uintptr_t)i);
  }
  if(snapshot->truncated)note(s,snapshot->nGpus*79,"Inventory exceeds display limits; additional devices may be omitted.",AMBER);
}
void gpuDetail(){
  int index=-1;for(int i=0;i<snapshot->nGpus;i++)if(!strcmp(snapshot->gpus[i].id,selectedGpuId))index=i;
  if(index<0){heading("GPU unavailable","",true);note(scroll(28),0,"This GPU is no longer in the current feed.",AMBER);return;}
  auto&g=snapshot->gpus[index];heading(g.name,g.status,true);label(content,gpuOwner(g),8,29,304,MUTED,&lv_font_montserrat_10);auto*s=scroll(46);cell(s,0,0,engineLoad(g)?"Engine load":"GPU utilization",number(g.utilization,1,"%"),CYAN);cell(s,1,0,!strcmp(g.kind,"integrated")?"Shared RAM":!strcmp(g.kind,"discrete")?"VRAM":"GPU RAM",gpuMemory(g),LAVENDER);cell(s,2,0,"Temperature",number(g.temp,0," °C"),AMBER);cell(s,3,0,"Power",number(g.power,1," W"),MINT);cell(s,4,0,"Graphics clock",number(g.graphicsMhz,0," MHz"));cell(s,5,0,"Memory clock",number(g.memoryMhz,0," MHz"));cell(s,6,0,"Fan",number(g.fan,0,"%"));cell(s,7,0,"Sample age",number(sampleAge(g.age,g.updated),0,"s"));int y=note(s,185,g.name,INK);
  if(g.error[0])y=note(s,y,g.error,AMBER);if(!strcmp(g.status,"inventory"))y=note(s,y,"Inventory is known; live GPU telemetry is unavailable.",AMBER);if(!strcmp(g.kind,"integrated"))y=note(s,y,"Integrated GPU uses shared system RAM. Dedicated GPU memory is not reported.");if(engineLoad(g))y=note(s,y,"Engine load is the busiest DRM engine, not total GPU utilization.");
  y=note(s,y,String("Vendor: ")+(g.vendor[0]?g.vendor:"--")+" • "+g.kind);y=note(s,y,"Driver: "+String(g.driver[0]?g.driver:"--"));note(s,y,"Device ID: "+String(g.id));
}
void alerts(){
  heading("Health & sources",String(snapshot->nAlerts)+" alerts",true);auto*s=scroll(28);int y=0;
  if(stale()){row(s,y,"Snapshot age",number(ageSeconds(),0,"s (stale)"),AMBER);y+=44;}
  for(int i=0;i<snapshot->nAlerts;i++){auto&a=snapshot->alerts[i];lv_point_t textSize;lv_text_get_size(&textSize,a.message,&lv_font_montserrat_12,0,0,276,LV_TEXT_FLAG_NONE);int h=35+textSize.y;auto*c=box(s,0,y,296,h);lv_obj_set_style_border_color(c,color(!strcmp(a.severity,"critical")?RED:AMBER),0);lv_obj_set_style_border_width(c,2,0);lv_obj_set_style_border_side(c,LV_BORDER_SIDE_LEFT,0);label(c,a.severity,10,7,278,!strcmp(a.severity,"critical")?RED:AMBER,&lv_font_montserrat_10);auto*l=label(c,a.message,10,26,276,INK);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=h+6;}
  if(!snapshot->nAlerts&&!stale()){row(s,y,"Active alerts","None",MINT);y+=44;}
  section(s,y,"PROXMOX FAULTS / LAST 24 HOURS");y+=18;
  row(s,y,"Segfaults",number(snapshot->segfaults24h,0),isfinite(snapshot->segfaults24h)?snapshot->segfaults24h>0?AMBER:MINT:MUTED);y+=44;row(s,y,"Fault events",number(snapshot->faultEvents24h,0),isfinite(snapshot->faultEvents24h)?snapshot->faultEvents24h>0?AMBER:MINT:MUTED);y+=44;row(s,y,"Last recorded fault",since(snapshot->lastFaultAt));y+=44;
  section(s,y,"RECENT FAULT HISTORY / "+String(snapshot->faultLookbackDays)+" DAYS");y+=18;
  if(!snapshot->nFaults){row(s,y,"Recent fault history",isfinite(snapshot->faultEvents24h)?"None":"Unavailable",isfinite(snapshot->faultEvents24h)?MINT:AMBER);y+=44;}
  for(int i=0;i<snapshot->nFaults;i++){auto&f=snapshot->faults[i];lv_point_t textSize;lv_text_get_size(&textSize,f.message,&lv_font_montserrat_12,0,0,276,LV_TEXT_FLAG_NONE);int h=36+textSize.y;auto*c=box(s,0,y,296,h);label(c,f.kind,10,8,164,!strcmp(f.kind,"segfault")?AMBER:RED,&lv_font_montserrat_10);auto*l=label(c,since(f.timestamp),180,8,106,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);l=label(c,f.message,10,28,276,INK);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=h+6;}
  section(s,y,"DATA SOURCES");y+=18;for(int i=0;i<snapshot->nSources;i++){auto&v=snapshot->sources[i];String message=v.error[0]?String(v.error):!v.enabled?"Source disabled by configuration":"Source reporting normally";lv_point_t textSize;lv_text_get_size(&textSize,message.c_str(),&lv_font_montserrat_10,0,0,276,LV_TEXT_FLAG_NONE);int h=42+textSize.y;auto*c=box(s,0,y,296,h);label(c,v.name,10,7,152,INK);double age=isfinite(v.age)?v.age+(millis()-snapshot->received)/1000.0:NAN;auto*l=label(c,!v.enabled?"DISABLED":v.ok?"OK  "+number(age,0,"s")+" ago":"UNAVAILABLE",164,7,121,!v.enabled?MUTED:v.ok?MINT:AMBER,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_RIGHT,0);auto*errorLabel=label(c,message,10,32,276,MUTED,&lv_font_montserrat_10);lv_label_set_long_mode(errorLabel,LV_LABEL_LONG_WRAP);lv_obj_set_height(errorLabel,LV_SIZE_CONTENT);y+=h+6;}
  if(!snapshot->nSources){row(s,y,"Source health","Unavailable",AMBER);y+=44;}
  if(snapshot->truncated)section(s,y,"Some entries exceed the display inventory limits.",AMBER);
}
void persistDisplay(){Preferences p;if(p.begin("display",false)){p.putUChar("brightness",brightness);p.putBool("rotate",autoRotate);bool savedLight=p.putBool("light",lightTheme)>0&&p.getBool("light",!lightTheme)==lightTheme;bool savedDim=p.putBool("dim",idleDim)>0&&p.getBool("dim",!idleDim)==idleDim;if(!savedLight||!savedDim)Serial.println("Display preference did not verify");p.end();}}
void brightnessChanged(lv_event_t*e){auto *input=lv_event_get_indev(e);if(input&&lv_indev_get_scroll_obj(input))return;brightness=lv_slider_get_value((lv_obj_t*)lv_event_get_target(e));boardBacklight(brightness);persistDisplay();}
void rotateChanged(lv_event_t*e){auto*sw=(lv_obj_t*)lv_event_get_user_data(e);if(!sw)sw=(lv_obj_t*)lv_event_get_target(e);autoRotate=!autoRotate;if(autoRotate)lv_obj_add_state(sw,LV_STATE_CHECKED);else lv_obj_remove_state(sw,LV_STATE_CHECKED);lastRotate=millis();persistDisplay();}
void dimChanged(lv_event_t*e){auto*sw=(lv_obj_t*)lv_event_get_user_data(e);if(!sw)sw=(lv_obj_t*)lv_event_get_target(e);idleDim=!idleDim;if(idleDim)lv_obj_add_state(sw,LV_STATE_CHECKED);else{lv_obj_remove_state(sw,LV_STATE_CHECKED);if(dimmed){dimmed=false;boardBacklight(brightness);}}lastTouch=millis();persistDisplay();}
void themeChanged(lv_event_t*e){bool light=uintptr_t(lv_event_get_user_data(e));if(light==lightTheme)return;useTheme(light);persistDisplay();render();}
void networkSetup(lv_event_t*){manualDemo=false;requestSetup=true;page=8;render();}
void exitDemo(lv_event_t*){manualDemo=false;page=0;render();}
void startDemo(lv_event_t*){manualDemo=true;makeDemo(*snapshot);page=0;historyCount=0;for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}render();}
void readManagement(){
  readConnectionSettings(connectionUi);readNodeConfiguration(nodeConfigUi);if(wifiSavePending&&!connectionUi.busy){if(connectionUi.saved)wifiDraft[1]=wifiDraft[3]=wifiDraft[4]="";wifiSavePending=false;}
  if((nodeSavePending||nodeDeletePending)&&!nodeConfigUi.busy){if(nodeConfigUi.saved){nodeDraft[2]="";nodeSavePending=nodeDeletePending=false;page=15;}else if(nodeConfigUi.message[0])nodeSavePending=nodeDeletePending=false;}
}
void openConnection(lv_event_t*){manualDemo=false;readSnapshot(*snapshot,net);readManagement();wifiDraft[0]=connectionUi.ssid;wifiDraft[1]="";wifiDraft[2]=connectionUi.endpoint;wifiDraft[3]=wifiDraft[4]="";wifiClearPassword=false;formMessage="";openPage(14);}
void openNodeSettings(lv_event_t*){manualDemo=false;readSnapshot(*snapshot,net);formMessage="";requestNodeConfiguration();openPage(15);}
void refreshNodes(lv_event_t*){requestNodeConfiguration();render();}
String&fieldDraft(){return fieldOrigin==14?wifiDraft[fieldIndex]:nodeDraft[fieldIndex];}
bool secretField(){return fieldOrigin==14?(fieldIndex==1||fieldIndex==3||fieldIndex==4):fieldIndex==2;}
void fieldChanged(lv_event_t*e){fieldDraft()=lv_textarea_get_text(lv_event_get_current_target_obj(e));if(fieldDraft().length()){if(fieldOrigin==14&&fieldIndex==1)wifiClearPassword=false;else if(fieldOrigin==16&&fieldIndex==2)nodeClearSecret=false;}}
void finishField(bool cancel){if(cancel){fieldDraft()=fieldOriginal;if(fieldOrigin==14&&fieldIndex==1)wifiClearPassword=fieldOriginalToggle;else if(fieldOrigin==16&&fieldIndex==2)nodeClearSecret=fieldOriginalToggle;}page=fieldOrigin;render();}
void fieldDone(lv_event_t*e){finishField(lv_event_get_code(e)==LV_EVENT_CANCEL);}
void clearField(lv_event_t*){lv_textarea_set_text(fieldArea,"");}
void editField(lv_event_t*e){fieldOrigin=page;fieldIndex=uintptr_t(lv_event_get_user_data(e));fieldOriginal=fieldDraft();fieldOriginalToggle=fieldOrigin==14?wifiClearPassword:nodeClearSecret;page=18;render();}
void keyboardInput(lv_event_t*e){auto*o=lv_event_get_current_target_obj(e);auto*input=lv_event_get_indev(e);lv_point_t point=tapOrigin;if(input)lv_indev_get_point(input,&point);int dx=point.x-tapOrigin.x,dy=point.y-tapOrigin.y;if(tapTarget!=o||tapDragged||consumeWakeTouch||dx*dx+dy*dy>=TAP_SLOP_PX*TAP_SLOP_PX)return;lv_keyboard_def_event_cb(e);if(keyboard)lv_buttonmatrix_set_button_ctrl_all(keyboard,lv_buttonmatrix_ctrl_t(LV_BUTTONMATRIX_CTRL_CLICK_TRIG|LV_BUTTONMATRIX_CTRL_NO_REPEAT));}
void fieldEditor(){
  heading(fieldOrigin==14?wifiFields[fieldIndex]:nodeFields[fieldIndex],"",true);auto*b=box(content,260,0,60,40,BG,0);label(b,"Clear",9,12,48,CYAN,&lv_font_montserrat_10);clickable(b,clearField);
  fieldArea=lv_textarea_create(content);lv_obj_set_pos(fieldArea,8,38);lv_obj_set_size(fieldArea,304,40);lv_textarea_set_one_line(fieldArea,true);const uint16_t wifiLimits[]={32,64,240,192,192},nodeLimits[]={63,240,256,32,8,8,8};lv_textarea_set_max_length(fieldArea,fieldOrigin==14?wifiLimits[fieldIndex]:nodeLimits[fieldIndex]);lv_textarea_set_password_mode(fieldArea,secretField());lv_textarea_set_password_show_time(fieldArea,0);if(fieldOrigin==16&&fieldIndex>=4)lv_textarea_set_accepted_chars(fieldArea,"0123456789.");lv_textarea_set_text(fieldArea,fieldDraft().c_str());lv_textarea_set_cursor_pos(fieldArea,LV_TEXTAREA_CURSOR_LAST);lv_obj_set_style_bg_color(fieldArea,color(CARD),0);lv_obj_set_style_text_color(fieldArea,color(INK),0);lv_obj_set_style_text_font(fieldArea,&lv_font_montserrat_14,0);lv_obj_set_style_border_color(fieldArea,color(CYAN),0);lv_obj_set_style_border_width(fieldArea,1,0);lv_obj_set_style_radius(fieldArea,7,0);lv_obj_set_style_pad_all(fieldArea,8,0);lv_obj_add_event_cb(fieldArea,fieldChanged,LV_EVENT_VALUE_CHANGED,nullptr);
  keyboard=lv_keyboard_create(lv_screen_active());lv_obj_set_align(keyboard,LV_ALIGN_TOP_LEFT);lv_obj_set_pos(keyboard,0,118);lv_obj_set_size(keyboard,320,122);lv_keyboard_set_textarea(keyboard,fieldArea);if(fieldOrigin==16&&fieldIndex>=4)lv_keyboard_set_mode(keyboard,LV_KEYBOARD_MODE_NUMBER);lv_keyboard_set_popovers(keyboard,false);lv_obj_remove_flag(keyboard,LV_OBJ_FLAG_SCROLLABLE);lv_obj_add_flag(keyboard,LV_OBJ_FLAG_PRESS_LOCK);lv_obj_set_style_bg_color(keyboard,color(BG),LV_PART_MAIN);lv_obj_set_style_border_width(keyboard,0,LV_PART_MAIN);lv_obj_set_style_pad_all(keyboard,3,LV_PART_MAIN);lv_obj_set_style_pad_row(keyboard,3,LV_PART_MAIN);lv_obj_set_style_pad_column(keyboard,2,LV_PART_MAIN);lv_obj_set_style_bg_color(keyboard,color(CARD),LV_PART_ITEMS);lv_obj_set_style_bg_color(keyboard,color(EDGE),LV_PART_ITEMS|LV_STATE_CHECKED);lv_obj_set_style_text_color(keyboard,color(INK),LV_PART_ITEMS|LV_STATE_CHECKED);lv_obj_set_style_bg_color(keyboard,color(CYAN),LV_PART_ITEMS|LV_STATE_PRESSED);lv_obj_set_style_text_color(keyboard,color(CARD),LV_PART_ITEMS|LV_STATE_PRESSED);lv_obj_set_style_bg_opa(keyboard,LV_OPA_COVER,LV_PART_ITEMS);lv_obj_set_style_text_color(keyboard,color(INK),LV_PART_ITEMS);lv_obj_set_style_text_font(keyboard,&lv_font_montserrat_12,LV_PART_ITEMS);lv_obj_set_style_border_width(keyboard,1,LV_PART_ITEMS);lv_obj_set_style_border_color(keyboard,color(EDGE),LV_PART_ITEMS);lv_obj_set_style_radius(keyboard,4,LV_PART_ITEMS);lv_buttonmatrix_set_button_ctrl_all(keyboard,lv_buttonmatrix_ctrl_t(LV_BUTTONMATRIX_CTRL_CLICK_TRIG|LV_BUTTONMATRIX_CTRL_NO_REPEAT));lv_obj_remove_event_cb(keyboard,lv_keyboard_def_event_cb);lv_obj_add_event_cb(keyboard,tapGuard,LV_EVENT_ALL,nullptr);lv_obj_add_event_cb(keyboard,keyboardInput,LV_EVENT_VALUE_CHANGED,nullptr);lv_obj_add_event_cb(keyboard,fieldDone,LV_EVENT_READY,nullptr);lv_obj_add_event_cb(keyboard,fieldDone,LV_EVENT_CANCEL,nullptr);
}
void formField(lv_obj_t*p,int y,const char*key,const String&value,uint8_t index){auto*c=box(p,0,y,296,48,CARD,7);label(c,key,9,7,271,MUTED,&lv_font_montserrat_10);label(c,value,9,24,265,INK);label(c,">",279,23,10,CYAN,&lv_font_montserrat_10);clickable(c,editField,(void*)(uintptr_t)index);}
void toggleWifiClear(lv_event_t*){wifiClearPassword=!wifiClearPassword;if(wifiClearPassword)wifiDraft[1]="";render();}
void saveConnection(lv_event_t*){
  if(wifiDraft[0].length()>32||wifiDraft[1].length()>64||wifiDraft[2].length()>240||wifiDraft[3].length()>192||wifiDraft[4].length()>192){formMessage="A field exceeds its byte limit.";render();return;}ConnectionUpdate update;strlcpy(update.ssid,wifiDraft[0].c_str(),sizeof(update.ssid));strlcpy(update.password,wifiDraft[1].c_str(),sizeof(update.password));strlcpy(update.endpoint,wifiDraft[2].c_str(),sizeof(update.endpoint));strlcpy(update.displayToken,wifiDraft[3].c_str(),sizeof(update.displayToken));strlcpy(update.setupToken,wifiDraft[4].c_str(),sizeof(update.setupToken));update.clearPassword=wifiClearPassword;if(requestConnectionUpdate(update)){wifiSavePending=true;formMessage="";}else formMessage="A settings save is already in progress.";render();
}
void connectionForm(){
  heading("Wi-Fi & collector","",true);auto*s=scroll(28);int y=0;if(formMessage.length())y=note(s,y,formMessage,RED);if(connectionUi.message[0])y=note(s,y,connectionUi.message,connectionUi.busy?CYAN:AMBER);formField(s,y,wifiFields[0],wifiDraft[0].length()?wifiDraft[0]:String("Enter Wi-Fi name"),0);y+=54;formField(s,y,wifiFields[1],wifiDraft[1].length()?String("•••• (new password)"):connectionUi.passwordSaved&&wifiDraft[0]==String(connectionUi.ssid)?String("Saved • blank keeps current"):String("Blank = open network"),1);y+=54;formField(s,y,wifiFields[2],wifiDraft[2].length()?wifiDraft[2]:String("Enter snapshot URL"),2);y+=54;formField(s,y,wifiFields[3],wifiDraft[3].length()?String("•••• (new token)"):connectionUi.displayTokenSaved?String("Saved • blank keeps current"):String("Enter read-only display token"),3);y+=54;formField(s,y,wifiFields[4],wifiDraft[4].length()?String("•••• (new setup token)"):connectionUi.setupTokenSaved?String("Paired • blank keeps current"):String("Optional: pair node management"),4);y+=54;
  auto*c=box(s,0,y,296,40);label(c,wifiClearPassword?"Clear saved Wi-Fi password: ON":"Clear saved Wi-Fi password: OFF",9,13,278,wifiClearPassword?AMBER:MUTED,&lv_font_montserrat_10);clickable(c,toggleWifiClear);y+=46;y=note(s,y,"Blank Wi-Fi password keeps it for the same network. Tokens stay saved only for the same collector URL; pair setup access separately.");smallButton(s,connectionUi.busy?"Saving...":"Save & connect",0,y,296,CYAN,saveConnection);y+=48;smallButton(s,"Advanced browser setup / HTTPS CA",0,y,296,MUTED,networkSetup);y+=48;note(s,y,"Passwords and tokens are masked and never read back.");
}
void beginNode(const ConfigNode&node,bool existing){editingNode=node;nodeExisting=existing;nodeClearSecret=false;nodeDraft[0]=node.name;nodeDraft[1]=node.url;nodeDraft[2]="";nodeDraft[3]=existing?String(node.id):String("");nodeDraft[4]=number(node.poll,1);nodeDraft[5]=number(node.timeout,1);nodeDraft[6]=number(node.ttl,1);formMessage="";openPage(16);}
void editNode(lv_event_t*e){size_t i=uintptr_t(lv_event_get_user_data(e));if(i<nodeConfigUi.count)beginNode(nodeConfigUi.nodes[i],true);}
void addNode(lv_event_t*){if(nodeConfigUi.count>=MAX_NODES)return;ConfigNode node;strlcpy(node.type,"klipper",sizeof(node.type));node.poll=5;node.timeout=3;node.ttl=15;beginNode(node,false);}
void setNodeType(lv_event_t*e){if(nodeExisting)return;int selected=uintptr_t(lv_event_get_user_data(e));if(selected==0&&nodeConfigUi.localEnabled){formMessage="Local Proxmox is already configured.";render();return;}strlcpy(editingNode.type,selected==0?"local-proxmox":selected==1?"klipper":"proxmox-feed",sizeof(editingNode.type));if(selected==0){strlcpy(editingNode.id,nodeConfigUi.localId,sizeof(editingNode.id));nodeDraft[0]=nodeConfigUi.localName;nodeDraft[3]=nodeConfigUi.localId;}else{editingNode.id[0]=0;nodeDraft[3]="";}formMessage="";render();}
void toggleNodeSecret(lv_event_t*){nodeClearSecret=!nodeClearSecret;if(nodeClearSecret)nodeDraft[2]="";render();}
double draftNumber(const String&s){char*end;double result=strtod(s.c_str(),&end);return end!=s.c_str()&&!*end&&isfinite(result)?result:NAN;}
void saveNode(lv_event_t*){
  if(!nodeConfigUi.available){formMessage="Load collector settings before saving.";render();return;}if(nodeDraft[0].length()>63||nodeDraft[1].length()>240||nodeDraft[2].length()>256||(!nodeExisting&&strcmp(editingNode.type,"local-proxmox")&&nodeDraft[3].length()>32)){formMessage="A node field exceeds its byte limit.";render();return;}NodeConfigDraft draft;draft.node=editingNode;strlcpy(draft.node.name,nodeDraft[0].c_str(),sizeof(draft.node.name));strlcpy(draft.node.url,nodeDraft[1].c_str(),sizeof(draft.node.url));if(!nodeExisting&&strcmp(draft.node.type,"local-proxmox"))strlcpy(draft.node.id,nodeDraft[3].c_str(),sizeof(draft.node.id));strlcpy(draft.secret,nodeDraft[2].c_str(),sizeof(draft.secret));draft.clearSecret=nodeClearSecret;draft.node.poll=draftNumber(nodeDraft[4]);draft.node.timeout=draftNumber(nodeDraft[5]);draft.node.ttl=draftNumber(nodeDraft[6]);if(strcmp(draft.node.type,"local-proxmox")&&(!isfinite(draft.node.poll)||!isfinite(draft.node.timeout)||!isfinite(draft.node.ttl))){formMessage="Enter valid polling, timeout and TTL numbers.";render();return;}if(requestNodeUpsert(draft,nodeConfigUi.version)){nodeSavePending=true;formMessage="";}else formMessage="Check the node ID, or wait for the active settings request.";render();
}
void confirmDelete(lv_event_t*){strlcpy(deleteNodeId,editingNode.id,sizeof(deleteNodeId));openPage(17);}
void deleteConfirmed(lv_event_t*){if(requestNodeDelete(deleteNodeId,nodeConfigUi.version)){nodeDeletePending=true;formMessage="";}else formMessage="Wait for the active settings request, then retry.";render();}
void cancelDelete(lv_event_t*){openPage(16);}
void nodeManager(){
  heading("Manage nodes",String(nodeConfigUi.count)+" / 4",true);auto*s=scroll(28);int y=0;if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,nodeConfigUi.busy?CYAN:AMBER);if(!nodeConfigUi.available){smallButton(s,"Load / refresh settings",0,y,296,CYAN,refreshNodes);y+=48;smallButton(s,"Pair setup token / Wi-Fi",0,y,296,MUTED,openConnection);return;}for(int i=0;i<nodeConfigUi.count;i++){auto&n=nodeConfigUi.nodes[i];auto*c=box(s,0,y,296,62,CARD,7);label(c,n.name,9,8,265,INK);label(c,">",280,24,9,CYAN);label(c,!strcmp(n.type,"local-proxmox")?"Local Proxmox":!strcmp(n.type,"klipper")?"Klipper":"Remote Proxmox feed",9,29,270,MUTED,&lv_font_montserrat_10);label(c,n.id,9,45,270,MUTED,&lv_font_montserrat_8);clickable(c,editNode,(void*)(uintptr_t)i);y+=68;}if(!nodeConfigUi.count)y=note(s,y,"No nodes configured. Add a monitor below.");if(nodeConfigUi.count<MAX_NODES){smallButton(s,"+ Add node",0,y,296,CYAN,addNode);y+=48;}smallButton(s,"Refresh configuration",0,y,296,MUTED,refreshNodes);
}
void nodeForm(){
  bool local=!strcmp(editingNode.type,"local-proxmox");heading(nodeExisting?"Edit node":"Add node","",true);auto*s=scroll(28);int y=0;if(formMessage.length())y=note(s,y,formMessage,RED);if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,nodeConfigUi.busy?CYAN:AMBER);if(!nodeExisting){const char*names[]={"Local","Klipper","PVE feed"};for(int i=0;i<3;i++){const char*t=i==0?"local-proxmox":i==1?"klipper":"proxmox-feed";auto*b=box(s,i*99,y,93,40,CARD,7);bool active=!strcmp(editingNode.type,t);lv_obj_set_style_border_color(b,color(active?CYAN:EDGE),0);label(b,names[i],7,13,81,active?CYAN:MUTED,&lv_font_montserrat_10);clickable(b,setNodeType,(void*)(uintptr_t)i);}y+=48;}formField(s,y,nodeFields[0],nodeDraft[0].length()?nodeDraft[0]:String("Enter display name"),0);y+=54;
  if(local){y=note(s,y,"Local Proxmox address: "+String(nodeConfigUi.localAddress));y=note(s,y,"Fixed ID: "+String(nodeConfigUi.localId));}else{formField(s,y,nodeFields[1],nodeDraft[1].length()?nodeDraft[1]:String(!strcmp(editingNode.type,"klipper")?"http://printer:7125":"http://collector:8765/api/v1/snapshot"),1);y+=54;formField(s,y,nodeFields[2],nodeDraft[2].length()?String("•••• (new service token)"):editingNode.hasSecret?String("Saved • keep for same address"):String(!strcmp(editingNode.type,"proxmox-feed")?"Other collector display token":"Optional Moonraker API key"),2);y+=54;if(nodeExisting)y=note(s,y,"Fixed ID: "+String(editingNode.id));else{formField(s,y,nodeFields[3],nodeDraft[3].length()?nodeDraft[3]:String("Blank creates ID from name"),3);y+=54;}section(s,y,"ADVANCED SAMPLING");y+=18;for(int i=4;i<7;i++){formField(s,y,nodeFields[i],nodeDraft[i],i);y+=54;}y=note(s,y,"TTL covers at least two polls. Saved service tokens stay only for the same address. The collector validates all ranges.");if(editingNode.hasSecret){auto*c=box(s,0,y,296,40);label(c,nodeClearSecret?"Clear saved service token: ON":"Clear saved service token: OFF",9,13,278,nodeClearSecret?AMBER:MUTED,&lv_font_montserrat_10);clickable(c,toggleNodeSecret);y+=46;}}
  smallButton(s,nodeConfigUi.busy?"Saving...":"Save node",0,y,296,CYAN,saveNode);y+=48;if(nodeExisting){smallButton(s,"Remove node",0,y,296,RED,confirmDelete);y+=48;}note(s,y,"Saving briefly restarts the monitoring collector.");
}
void deleteScreen(){heading("Remove node?","",true);auto*s=scroll(28);int y=note(s,0,editingNode.name,INK);y=note(s,y,String(deleteNodeId));y=note(s,y,"Remove this node from desk monitoring? You can add it again from Settings.");if(formMessage.length())y=note(s,y,formMessage,RED);if(nodeConfigUi.message[0])y=note(s,y,nodeConfigUi.message,AMBER);smallButton(s,nodeConfigUi.busy?"Removing...":"Remove this node",0,y,296,RED,deleteConfirmed);y+=48;smallButton(s,"Keep node",0,y,296,CYAN,cancelDelete);}
void settings(){
  auto*s=scroll();int y=0;section(s,y,"DISPLAY SETTINGS");y+=20;
  auto*c=box(s,0,y,296,48);label(c,"Appearance",10,16,98,INK);for(int i=0;i<2;i++){bool light=i==0;auto*b=box(c,110+i*88,4,82,40,light==lightTheme?(lightTheme?CARD:0x1a3d49):CARD,7);if(light==lightTheme)lv_obj_set_style_border_color(b,color(CYAN),0);label(b,light?"White":"Dark",12,13,60,light==lightTheme?CYAN:MUTED);clickable(b,themeChanged,(void*)(uintptr_t)light);}y+=56;
  c=box(s,0,y,296,68);label(c,"Brightness",10,10,180,INK,&lv_font_montserrat_14);auto*slider=lv_slider_create(c);lv_obj_add_flag(slider,LV_OBJ_FLAG_PRESS_LOCK);lv_obj_set_pos(slider,14,43);lv_obj_set_size(slider,266,8);lv_obj_set_ext_click_area(slider,16);lv_slider_set_range(slider,10,100);lv_slider_set_value(slider,brightness,LV_ANIM_OFF);lv_obj_set_style_bg_color(slider,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(slider,color(CYAN),LV_PART_INDICATOR);lv_obj_set_style_bg_color(slider,color(CYAN),LV_PART_KNOB);lv_obj_set_style_border_width(slider,0,LV_PART_MAIN);lv_obj_set_style_border_width(slider,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(slider,0,LV_PART_KNOB);lv_obj_add_event_cb(slider,brightnessChanged,LV_EVENT_RELEASED,nullptr);y+=76;
  c=box(s,0,y,296,48);label(c,"Rotate pages every 15s",10,15,225,INK);auto*sw=lv_switch_create(c);lv_obj_set_pos(sw,242,12);lv_obj_set_size(sw,44,24);lv_obj_remove_flag(sw,LV_OBJ_FLAG_CHECKABLE);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(sw,color(CARD),LV_PART_KNOB);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_INDICATOR);lv_obj_set_style_bg_color(sw,color(CYAN),LV_PART_INDICATOR|LV_STATE_CHECKED);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_MAIN);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_INDICATOR);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_KNOB);lv_obj_set_style_border_width(sw,0,LV_PART_MAIN);lv_obj_set_style_border_width(sw,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(sw,0,LV_PART_KNOB);if(autoRotate)lv_obj_add_state(sw,LV_STATE_CHECKED);clickable(sw,rotateChanged);clickable(c,rotateChanged,sw);y+=56;
  c=box(s,0,y,296,48);label(c,"Dim after 2 min",10,15,225,INK);sw=lv_switch_create(c);lv_obj_set_pos(sw,242,12);lv_obj_set_size(sw,44,24);lv_obj_remove_flag(sw,LV_OBJ_FLAG_CHECKABLE);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_MAIN);lv_obj_set_style_bg_color(sw,color(CARD),LV_PART_KNOB);lv_obj_set_style_bg_color(sw,color(EDGE),LV_PART_INDICATOR);lv_obj_set_style_bg_color(sw,color(CYAN),LV_PART_INDICATOR|LV_STATE_CHECKED);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_MAIN);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_INDICATOR);lv_obj_set_style_bg_opa(sw,LV_OPA_COVER,LV_PART_KNOB);lv_obj_set_style_border_width(sw,0,LV_PART_MAIN);lv_obj_set_style_border_width(sw,0,LV_PART_INDICATOR);lv_obj_set_style_border_width(sw,0,LV_PART_KNOB);if(idleDim)lv_obj_add_state(sw,LV_STATE_CHECKED);clickable(sw,dimChanged);clickable(c,dimChanged,sw);y+=56;section(s,y,"Touch wakes when dimmed. Off stays bright.");y+=22;smallButton(s,"Nodes & monitoring",0,y,296,CYAN,openNodeSettings);y+=48;smallButton(s,"Wi-Fi & collector setup",0,y,296,CYAN,openConnection);y+=48;
  if(manualDemo){smallButton(s,"Exit demo",0,y,296,AMBER,exitDemo);y+=48;}else{smallButton(s,"Preview demo (sample data)",0,y,296,LAVENDER,startDemo);y+=48;}
  row(s,y,"Display IP",net.ip[0]?String(net.ip):String("Not connected"));y+=44;row(s,y,"Network",net.message);y+=44;section(s,y,"Glimdock • open desk monitor",LAVENDER);y+=20;section(s,y,"Waveshare ESP32-S3 Touch LCD 2.8 V1 / schema 1");
}
void setupScreen(){
  auto*s=scroll();label(s,"SET UP GLIMDOCK",4,2,292,LAVENDER,&lv_font_montserrat_14);label(s,net.setup?"Join the Wi-Fi network below":net.message[0]?String(net.message):String("Starting configuration network..."),4,25,292,MUTED);int y=47;smallButton(s,"Configure on this display",0,y,296,CYAN,openConnection);y+=48;
  row(s,y,"Wi-Fi network",glimdock::SETUP_SSID,CYAN);y+=44;row(s,y,"Setup password",net.apPassword[0]?String(net.apPassword):String("Preparing..."),INK);y+=44;row(s,y,"Open browser","192.168.4.1",MINT);y+=44;
  auto*l=label(s,"Enter your Wi-Fi credentials, collector URL and dedicated display token. The default collector is 192.0.2.10:8765.",4,y,292,MUTED);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);y+=64;
  smallButton(s,"Preview demo (sample data)",0,y,296,LAVENDER,startDemo);
}
void noData(){
  auto*c=box(content,8,0,304,166);label(c,net.loading?"LOADING NODE":net.wifi?"WAITING FOR COLLECTOR":"CONNECTING",12,20,280,CYAN,&lv_font_montserrat_18);auto*l=label(c,net.message[0]?String(net.message):String("Waiting for network worker"),12,51,280,MUTED);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_height(l,LV_SIZE_CONTENT);smallButton(c,"Wi-Fi & collector setup",12,86,280,CYAN,openConnection);label(c,"No live readings received yet",12,142,280,MUTED,&lv_font_montserrat_10);
}
void render(){
  if(!uiReady)return;readManagement();if(keyboard){lv_obj_delete(keyboard);keyboard=nullptr;fieldArea=nullptr;}if(page==18)lv_obj_add_flag(tabBar,LV_OBJ_FLAG_HIDDEN);else lv_obj_remove_flag(tabBar,LV_OBJ_FLAG_HIDDEN);int oldScroll=scrollArea?lv_obj_get_scroll_y(scrollArea):0;static uint8_t previousPage=255;lv_obj_clean(content);scrollArea=nullptr;header();
  if(page==18)fieldEditor();else if(page==17)deleteScreen();else if(page==16)nodeForm();else if(page==15)nodeManager();else if(page==14)connectionForm();else if(page==13)nodePicker();else if(page==8)setupScreen();else if(page==7)settings();else if(!snapshot->valid)noData();else if(printerNode()){switch(page){case 1:printerJob();break;case 2:printerTemps();break;case 3:case 6:printerHealth();break;default:printerOverview();break;}}else switch(page){case 0:overview();break;case 1:guests();break;case 2:storage();break;case 3:sensors();break;case 4:guestDetail();break;case 5:powerDetail();break;case 6:alerts();break;case 9:diskDetail();break;case 10:memoryDetail();break;case 11:gpuList();break;case 12:gpuDetail();break;}
  if(scrollArea&&previousPage==page)lv_obj_scroll_to_y(scrollArea,oldScroll,LV_ANIM_OFF);previousPage=page;lastRender=millis();
}
void buildUI(){
  lv_obj_t*screen=lv_screen_active();lv_obj_set_style_bg_color(screen,color(BG),0);lv_obj_set_style_pad_all(screen,0,0);lv_obj_remove_flag(screen,LV_OBJ_FLAG_SCROLLABLE);
  auto*home=box(screen,0,0,104,34,BG,0);lv_obj_set_style_bg_opa(home,LV_OPA_TRANSP,0);headerMark=brandMark(home,4,8);headerTitle=brandWordmark(home,27,4);clickable(home,go,(void*)0);nodeButton=box(screen,104,0,106,34,BG,0);headerHost=label(nodeButton,"Choose node",5,11,84,MUTED,&lv_font_montserrat_10);label(nodeButton,LV_SYMBOL_DOWN,90,12,12,CYAN,&lv_font_montserrat_8);clickable(nodeButton,chooseNode);
  statePill=box(screen,214,8,53,18,CARD,6);headerState=label(statePill,"OFFLINE",1,4,51,AMBER,&lv_font_montserrat_8);lv_obj_set_style_text_align(headerState,LV_TEXT_ALIGN_CENTER,0);clickable(statePill,go,(void*)6);settingsButton=box(screen,272,0,48,34,BG,0);label(settingsButton,LV_SYMBOL_SETTINGS,14,10,30,MUTED,&lv_font_montserrat_14);clickable(settingsButton,go,(void*)7);headerLine=box(screen,0,33,320,1,EDGE,0);lv_obj_set_style_border_width(headerLine,0,0);
  content=box(screen,0,34,320,166,BG,0);
  tabBar=box(screen,0,200,320,40,CARD,0);lv_obj_set_style_border_side(tabBar,LV_BORDER_SIDE_TOP,0);
  for(int i=0;i<4;i++){tabs[i]=box(tabBar,i*80,0,80,40,CARD,0);lv_obj_set_style_border_width(tabs[i],0,0);icon(tabs[i],16+i,33,6,14,MUTED);auto*l=label(tabs[i],titles[i],2,23,76,MUTED,&lv_font_montserrat_10);lv_obj_set_style_text_align(l,LV_TEXT_ALIGN_CENTER,0);auto*rule=box(tabs[i],27,0,26,2,CYAN,0);lv_obj_set_style_border_width(rule,0,0);clickable(tabs[i],go,(void*)(uintptr_t)i);}
  uiReady=true;render();
}
void captureHistory(){for(int i=0;i<31;i++){cpuHistory[i]=cpuHistory[i+1];ramHistory[i]=ramHistory[i+1];}cpuHistory[31]=snapshot->cpu;ramHistory[31]=memoryPct(snapshot->memUsed,snapshot->memTotal);historyCount=min(32,int(historyCount+1));}
}
void noteTouch(bool pressed){if(!uiReady)return;if(pressed){lastTouch=millis();lastRotate=millis();if(dimmed){dimmed=false;consumeWakeTouch=true;boardBacklight(brightness);}}else if(consumeWakeTouch)consumeWakeTouch=false;}
#ifndef HOMELAB_NATIVE_PREVIEW
void setup(){
  boardHoldPower();
  Serial.begin(115200);boardInit();boardLvglInit();Preferences p;p.begin("display",true);brightness=constrain(p.getUChar("brightness",80),10,100);autoRotate=p.getBool("rotate",false);idleDim=p.getBool("dim",false);useTheme(p.getBool("light",true));p.end();
  Serial.printf("Glimdock boot: reset=%d; power latch=on; gyro/RTC/audio=off\n",int(esp_reset_reason()));
  snapshot=(Snapshot*)heap_caps_malloc(sizeof(Snapshot),MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
  if(!snapshot){auto*l=lv_label_create(lv_screen_active());lv_label_set_text(l,"PSRAM unavailable\nCheck V1 board configuration");lv_obj_center(l);boardBacklight(80);return;}new(snapshot)Snapshot{};
  for(int i=0;i<32;i++){cpuHistory[i]=NAN;ramHistory[i]=NAN;}startNetwork();lastTouch=lastRotate=millis();buildUI();boardBacklight(brightness);
}
void loop(){
  if(!snapshot){lv_timer_handler();delay(10);return;}uint32_t now=millis();
  if(now-lastPoll>=500){lastPoll=now;if(manualDemo)makeDemo(*snapshot);else readSnapshot(*snapshot,net);
    if(strcmp(trackedNodeId,snapshot->node.id)||(net.loading&&!snapshot->valid&&lastSequence)){uint8_t previous=page;resetNodeView();if(previous==7||previous==8||previous>=14)page=previous;}
    if(!net.configured&&!manualDemo&&page!=7&&page<13)page=8;
    else if(net.configured&&!net.setup&&page==8&&!manualDemo)page=0;
    if(snapshot->valid&&!printerNode()&&(manualDemo||snapshot->sequence!=lastSequence)){captureHistory();lastSequence=snapshot->sequence;}
    bool changed=net.revision!=lastNetRevision;lastNetRevision=net.revision;
    if(page!=7&&page!=18&&now-lastTouch>500&&!interactionBusy()&&(changed||now-lastRender>=1000))render();else header();
  }
  if(autoRotate&&page<4&&snapshot->valid&&!interactionBusy()&&now-lastRotate>HOMELAB_AUTO_PAGE_MS){openPage((page+1)%4);lastRotate=now;}
  if(idleDim&&!dimmed&&now-lastTouch>HOMELAB_IDLE_DIM_MS){dimmed=true;boardBacklight(10);}
  lv_timer_handler();delay(5);
}

#else
void homelabPreviewInit(Snapshot* data,NetworkState* state){snapshot=data;net=*state;strlcpy(trackedNodeId,data->node.id,sizeof(trackedNodeId));brightness=80;lastTouch=millis();lastRotate=millis();for(int i=0;i<32;i++){cpuHistory[i]=data->demo?16+i%8:NAN;ramHistory[i]=data->demo?55+i%4:NAN;}cpuHistory[31]=data->cpu;ramHistory[31]=memoryPct(data->memUsed,data->memTotal);buildUI();}
void homelabPreviewShow(uint8_t selected){if(selected==14){openConnection(nullptr);return;}if(selected==15){openNodeSettings(nullptr);return;}if(selected==16||selected==17){readManagement();ConfigNode n=nodeConfigUi.count>1?nodeConfigUi.nodes[1]:nodeConfigUi.count?nodeConfigUi.nodes[0]:ConfigNode{};beginNode(n,nodeConfigUi.count>0);if(selected==17){strlcpy(deleteNodeId,n.id,sizeof(deleteNodeId));page=17;render();}return;}if(selected==18){openConnection(nullptr);fieldOrigin=14;fieldIndex=4;fieldOriginal=wifiDraft[fieldIndex];page=18;render();return;}if(selected==11)gpuListOrigin=5;if(selected==4&&snapshot->nGuests)selectedGuestId=snapshot->guests[0].id;if(selected==9&&snapshot->nDisks)strlcpy(selectedDiskName,snapshot->disks[0].name,sizeof(selectedDiskName));if(selected==12&&snapshot->nGpus){strlcpy(selectedGpuId,snapshot->gpus[0].id,sizeof(selectedGpuId));gpuReturnPage=11;}detailOrigin=selected==4?1:selected==9?2:0;page=selected;render();}
void homelabPreviewGuest(int id){selectedGuestId=id;detailOrigin=1;page=4;render();}
void homelabPreviewRefresh(){render();}
void homelabPreviewPoll(){readSnapshot(*snapshot,net);if(strcmp(trackedNodeId,snapshot->node.id)||(net.loading&&!snapshot->valid&&lastSequence))resetNodeView();render();}
uint8_t homelabPreviewHistoryCount(){return historyCount;}
void homelabPreviewGpu(uint8_t index){if(index<snapshot->nGpus)strlcpy(selectedGpuId,snapshot->gpus[index].id,sizeof(selectedGpuId));gpuReturnPage=11;detailOrigin=0;page=12;render();}
void homelabPreviewScroll(int y){if(scrollArea)lv_obj_scroll_to_y(scrollArea,y,LV_ANIM_OFF);}
void homelabPreviewSensorFilter(uint8_t kind){sensorFilter=min(5,int(kind));page=3;render();}
void homelabPreviewTheme(bool light){useTheme(light);render();}
uint8_t homelabPreviewPage(){return page;}
uint8_t homelabPreviewFilter(){return sensorFilter;}
bool homelabPreviewLightTheme(){return lightTheme;}
bool homelabPreviewBusy(){return interactionBusy();}
bool homelabPreviewAutoRotate(){return autoRotate;}
bool homelabPreviewIdleDim(){return idleDim;}
uint8_t homelabPreviewBrightness(){return brightness;}
#endif
