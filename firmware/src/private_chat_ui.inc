// The private reader has its own transient model; it never enters snapshots.
struct PrivateRequest {int64_t after=-1,order=-1,offset=-1;};
char privateSession[97]{},privateCursor[513]{},privateCursorBack[16][513]{};
uint8_t privateCursorDepth=0,privateMessageDepth=0;
PrivateRequest privateRequest,privateMessageBack[64];
bool privateLabels=false;
char privateTask[65]{},privateSystem[65]{};uint8_t privateReturnPage=7;
void privateWipeLabels(lv_obj_t*o){
  if(!privateLabels||!o)return;
  if(lv_obj_check_type(o,&lv_label_class)){char*text=const_cast<char*>(lv_label_get_text(o));if(text)privatechat::wipe(text,strlen(text));}
  for(uint32_t i=0;i<lv_obj_get_child_count(o);i++)privateWipeLabels(lv_obj_get_child(o,i));
}
void privateResetUi(){
  privateWipeLabels(content);privateLabels=false;
  if(privateChat){privatechat::wipe(privateChat,sizeof(*privateChat));new(privateChat)privatechat::Model{};}
  privatechat::wipe(privateSession,sizeof(privateSession));privatechat::wipe(privateCursor,sizeof(privateCursor));privatechat::wipe(privateCursorBack,sizeof(privateCursorBack));privatechat::wipe(privateMessageBack,sizeof(privateMessageBack));privateRequest=PrivateRequest{};privateCursorDepth=privateMessageDepth=0;privateState=privatechat::State{};privateTask[0]=privateSystem[0]=0;privateReturnPage=7;
}
void privateRead(){if(privateChat)readPrivateChat(*privateChat,privateState);}
void privateLoadSessions(){requestPrivateSessions(privateCursor,privateTask,privateSystem);privateRead();privateNewRequest=true;render();}
void privateLoadMessages(){requestPrivateMessages(privateSession,privateRequest.after,privateRequest.order,privateRequest.offset);privateRead();privateNewRequest=true;render();}
void privateOpen(lv_event_t*){privateTask[0]=privateSystem[0]=0;privateReturnPage=7;agentsWorkspace=true;setViewPage(PRIVATE_SESSIONS);privateCursor[0]=0;privateCursorDepth=privateMessageDepth=0;privateRead();if(privateChatConfigured())privateLoadSessions();else render();}
void privateOpenTask(lv_event_t*){if(!operations||page!=OPS_TASK||!privatechat::validId(selectedOperation,64)||!privatechat::validId(selectedOperationSystem,64))return;strlcpy(privateTask,selectedOperation,sizeof(privateTask));strlcpy(privateSystem,selectedOperationSystem,sizeof(privateSystem));privateReturnPage=OPS_TASK;agentsWorkspace=true;setViewPage(PRIVATE_SESSIONS);privateCursor[0]=0;privateCursorDepth=privateMessageDepth=0;privateRead();if(privateChatConfigured())privateLoadSessions();else render();}
void privateSelectIndex(size_t i){if(!privateChat||i>=privateChat->nSessions)return;strlcpy(privateSession,privateChat->sessions[i].id,sizeof(privateSession));privateRequest=PrivateRequest{};privateMessageDepth=0;setViewPage(PRIVATE_MESSAGES);privateLoadMessages();}
void privateSelect(lv_event_t*e){privateSelectIndex(uintptr_t(lv_event_get_user_data(e)));}
void privateRememberMessage(){if(privateMessageDepth==64){memmove(privateMessageBack,privateMessageBack+1,63*sizeof(PrivateRequest));privateMessageDepth=63;}privateMessageBack[privateMessageDepth++]=privateRequest;}
void privateNext(lv_event_t*){if(!privateChat||!privateChat->valid)return;
  if(page==PRIVATE_SESSIONS){if(!privateChat->hasMore||!privateChat->nextCursor[0])return;if(privateCursorDepth==16){memmove(privateCursorBack,privateCursorBack+1,15*sizeof(privateCursorBack[0]));privateCursorDepth=15;}strlcpy(privateCursorBack[privateCursorDepth++],privateCursor,sizeof(privateCursor));strlcpy(privateCursor,privateChat->nextCursor,sizeof(privateCursor));privateLoadSessions();}
  else if(privateChat->hasMore&&privateChat->nextAfter>=0){privateRememberMessage();privateRequest={privateChat->nextAfter,-1,-1};privateLoadMessages();}
}
void privatePrevious(lv_event_t*){if(page==PRIVATE_SESSIONS&&privateCursorDepth){strlcpy(privateCursor,privateCursorBack[--privateCursorDepth],sizeof(privateCursor));privatechat::wipe(privateCursorBack[privateCursorDepth],sizeof(privateCursorBack[0]));privateLoadSessions();}else if(page==PRIVATE_MESSAGES&&privateMessageDepth){privateRequest=privateMessageBack[--privateMessageDepth];privateMessageBack[privateMessageDepth]=PrivateRequest{};privateLoadMessages();}}
void privateContinue(lv_event_t*e){size_t i=uintptr_t(lv_event_get_user_data(e));if(!privateChat||i>=privateChat->nMessages)return;auto&m=privateChat->messages[i];if(m.nextTextOffset<0)return;privateRememberMessage();privateRequest={privateRequest.after,int64_t(m.order),m.nextTextOffset};privateLoadMessages();}
void privateRetry(lv_event_t*){if(page==PRIVATE_SESSIONS)privateLoadSessions();else privateLoadMessages();}
void privateBack(){if(page==PRIVATE_MESSAGES){setViewPage(PRIVATE_SESSIONS);privateSession[0]=0;privateMessageDepth=0;privateLoadSessions();}else openPage(privateReturnPage);}
void privateHeader(){feedStatus=privateChat&&privateChat->demo?"DEMO":"PRIVATE";feedStatusColor=privateState.loading?AMBER:LAVENDER;lv_label_set_text(headerHost,"Chat history");lv_obj_invalidate(headerTitle);}
String privateTime(double value){if(!isfinite(value))return "Time unavailable";struct tm local{};if(!displayClockTm(time_t(value),clockZone,local))return "Time unavailable";char text[24];strftime(text,sizeof(text),"%d %b %H:%M",&local);return String(text);}
void privateWipeString(String&text){privatechat::wipe(const_cast<char*>(text.c_str()),text.length());}
lv_obj_t*privateLabel(lv_obj_t*p,String text,int x,int y,int w,uint32_t fg=INK,const lv_font_t*font=&lv_font_montserrat_12){auto*l=label(p,text,x,y,w,fg,font);privateWipeString(text);return l;}
int privateWrapped(lv_obj_t*p,int x,int y,int w,String text,uint32_t fg,const lv_font_t*font){lv_point_t size;lv_text_get_size(&size,text.c_str(),font,0,2,w,LV_TEXT_FLAG_NONE);auto*l=privateLabel(p,text,x,y,w,fg,font);lv_label_set_long_mode(l,LV_LABEL_LONG_WRAP);lv_obj_set_style_text_line_space(l,2,0);lv_obj_set_height(l,LV_SIZE_CONTENT);privateWipeString(text);return y+size.y;}
int privateNote(lv_obj_t*p,int y,String text,uint32_t fg=MUTED){int next=privateWrapped(p,7,y,LIST_W-14,text,fg,&lv_font_montserrat_10)+8;privateWipeString(text);return next;}
// A small, non-executing renderer: paragraphs, headings and fenced code blocks.
// URLs/HTML/tool commands are always plain text; no remote assets are loaded.
int privateBody(lv_obj_t*p,int y,int w,const char*text){
  String body(text),paragraph;paragraph.reserve(body.length());bool code=false;
  auto flush=[&](){if(!paragraph.length())return;int start=y;if(code){y=privateWrapped(p,10,y+7,w-20,paragraph,INK,&lv_font_montserrat_10)+7;auto*b=box(p,5,start,w-10,y-start,lightTheme?BG:DARK_SELECTED,5);lv_obj_move_background(b);}else y=privateWrapped(p,10,y,w-20,paragraph,INK,&lv_font_montserrat_12);y+=8;privateWipeString(paragraph);paragraph="";};
  size_t at=0;while(at<body.length()){int end=body.indexOf('\n',at);if(end<0)end=body.length();String line=body.substring(at,end);at=end+1;
    if(line.startsWith("```")){flush();code=!code;privateWipeString(line);continue;}
    if(!line.length()){flush();continue;}
    if(!code&&line.startsWith("#")){flush();int n=0;while(n<int(line.length())&&line[n]=='#')n++;String title=line.substring(n);title.trim();y=privateWrapped(p,10,y,w-20,title,LAVENDER,&lv_font_montserrat_12)+8;privateWipeString(title);privateWipeString(line);continue;}
    if(paragraph.length())paragraph+='\n';paragraph+=line;privateWipeString(line);
  }flush();privateWipeString(paragraph);privateWipeString(body);return y;
}
void privatePager(lv_obj_t*s,int y,bool previous,bool next,const char*nextLabel){int w=(LIST_W-8)/2;if(previous)smallButton(s,"Previous page",0,y,w,MUTED,privatePrevious);if(next)smallButton(s,nextLabel,previous?w+8:0,y,previous?w:LIST_W,CYAN,privateNext);}
void privateRender(){
  privateLabels=true;String state=privateChat&&privateChat->demo?"DEMO":"PRIVATE";
  heading(page==PRIVATE_SESSIONS?(privateTask[0]?"Task conversations":"Conversations"):"Conversation",state,true);auto*s=scroll(PORTRAIT?40:28);int y=0;
  if(!privateChat){privateNote(s,y,"Private reader memory unavailable.",AMBER);return;}
  if(!privateState.configured){y=privateNote(s,y,"Pair a private conversation relay over USB. Each display uses its own revocable credential and verifies the relay's HTTPS certificate.");privateNote(s,y,"Conversation text stays separate from monitoring data.",LAVENDER);return;}
  if(privateState.loading){privateNote(s,y,"Loading private conversation page...",LAVENDER);return;}
  if(!privateChat->valid){y=privateNote(s,y,privateState.message,AMBER);smallButton(s,"Try again",0,y,LIST_W,CYAN,privateRetry);return;}
  if(privateChat->demo)y=privateNote(s,y,"Sample conversations • no private archive access",LAVENDER);
  if(page==PRIVATE_SESSIONS){
    if(!privateChat->nSessions)y=privateNote(s,y,privateTask[0]?"No archived conversation is linked to this task yet.":"No conversations available.");
    for(int i=0;i<privateChat->nSessions;i++){auto&a=privateChat->sessions[i];auto*c=box(s,0,y,LIST_W,62,CARD,8);privateLabel(c,a.title[0]?a.title:"Untitled conversation",10,9,LIST_W-28,INK,&lv_font_montserrat_12);privateLabel(c,String(a.provider)+" • "+a.node,10,28,LIST_W-22,MUTED,&lv_font_montserrat_10);privateLabel(c,privateTime(a.updatedAt)+" • "+(a.messageCount>=0?String(a.messageCount)+" messages":String("Count unavailable")),10,45,LIST_W-22,MUTED,&lv_font_montserrat_8);privateLabel(c,">",LIST_W-19,10,12,LAVENDER);clickable(c,privateSelect,(void*)(uintptr_t)i);y+=70;}
    privatePager(s,y,privateCursorDepth>0,privateChat->hasMore,"More conversations");
  }else{
    y=privateWrapped(s,5,y,LIST_W-10,privateChat->selectedSessionTitle,INK,&lv_font_montserrat_12)+7;y=privateNote(s,y,String(privateChat->selectedSessionProvider)+" • "+privateChat->selectedSessionMachine);
    if(!privateChat->nMessages)y=privateNote(s,y,"No messages on this page.");
    for(int i=0;i<privateChat->nMessages;i++){auto&m=privateChat->messages[i];bool user=!strcmp(m.role,"user");int x=user?14:0,w=LIST_W-14;auto*c=box(s,x,y,w,40,CARD,9);lv_obj_set_style_border_color(c,color(user?LAVENDER:CYAN),0);String role=user?"YOU":!strcmp(m.role,"assistant")?"ASSISTANT":String(m.role);privateLabel(c,role,10,9,w-20,user?LAVENDER:CYAN,&lv_font_montserrat_10);privateLabel(c,privateTime(m.timestamp),10,26,w-20,MUTED,&lv_font_montserrat_8);int h=43;if(m.textOffset>0)h=privateWrapped(c,10,h,w-20,"Continued message",MUTED,&lv_font_montserrat_10)+8;if(m.redactions>0)h=privateWrapped(c,10,h,w-20,String(m.redactions)+" redaction(s)",MUTED,&lv_font_montserrat_8)+8;h=privateBody(c,h,w,m.text);
      if(m.nextTextOffset>=0){smallButton(c,"Read more of this message",8,h,w-16,LAVENDER,privateContinue,(void*)(uintptr_t)i);h+=48;}else if(m.truncated)h=privateWrapped(c,10,h,w-20,"More text is unavailable from this source.",AMBER,&lv_font_montserrat_10)+8;
      lv_obj_set_height(c,h+4);y+=h+14;
    }
    privatePager(s,y,privateMessageDepth>0,privateChat->hasMore,"Next messages");
    y+=48;privateNote(s,y,"Read-only • leave this view to clear conversation text",MUTED);
  }

}
