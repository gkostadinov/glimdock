// Entirely fictional, in-memory conversations. No private reader is accessed.
export function createPrivateChatDemo() {
  const now=Math.floor(Date.now()/1000);
  const names=['Tune a backup schedule','Build an agent adapter','Review a deployment','Explain a sensor reading','Plan a maintenance window','Test a conversation reader','Document a printer setup','Investigate an alert'];
  const sessions=Array.from({length:29},(_,i)=>({id:(i+1).toString(16).padStart(64,'0'),title:names[i%names.length]+(i>=8?' '+(1+Math.floor(i/8)):''),machine:['demo-laptop','demo-server','demo-workstation'][i%3],provider:['Codex','Claude','Gemini'][i%3],last_message_at:now-i*240,message_count:18}));
  const tasks=['design','publish','metrics','audit','build','docs','tests','checks','guide','review-input','queue-a','queue-b'];
  const taskSessions=(task,system)=>system==='shared-hub'&&tasks.includes(task)?sessions.filter((_,i)=>i%tasks.length===tasks.indexOf(task)):[];
  const longText='# Implementation notes\n\n'+('This is fictional demonstration text. The display requests a bounded part of a longer message and lets you continue reading. It keeps the source order and clears the page when you leave the reader.\n\n').repeat(15)+'End of the long demonstration message.';
  const text=[
    'Can you help me review our backup schedule?',
    '# Backup review\n\nThe nightly backup completed. I checked the retention policy and the last restore test.\n\n- Keep seven daily copies.\n- Keep four weekly copies.\n- Verify one restore every month.',
    'Show me an example configuration.',
    'A small example:\n\n```json\n{\n  "schedule": "nightly",\n  "retention_days": 7,\n  "verify_restore": true\n}\n```\n\nThis example uses fictional values. Review it before adapting it to your system.',
    'Can you explain the implementation details?',longText,
    'Do we need to change anything else?',
    'The monitoring setup is ready. Add a clear source timestamp and report unavailable readings honestly.\n\nThe conversation reader is read-only. It does not run commands from messages.',
  ];
  const chunk=(message,offset=0)=>{const points=Array.from(message.text),part=points.slice(offset,offset+1900).join(''),next=offset+Array.from(part).length;return {...message,text:part,text_offset:offset,text_characters:points.length,next_text_offset:next<points.length?next:null,redactions:0};};
  return async request=>{
    const base={schema_version:'private-chat/v1',demo:true};
    if(request.kind===1){const start=request.cursor?Number(request.cursor.replace(/^page-/,'')):0;if(!Number.isInteger(start)||start<0)throw Error('Invalid sample cursor');const selected=request.task?taskSessions(request.task,request.system):sessions,items=selected.slice(start,start+12),next=start+items.length;return {...base,...(request.task?{task_id:request.task,system_id:request.system}:{}),items,total:selected.length,has_more:next<selected.length,next_cursor:next<selected.length?'page-'+next:null};}
    const session=sessions.find(s=>s.id===request.session);if(!session)throw Error('Unknown sample conversation');
    const messages=Array.from({length:session.message_count},(_,order)=>({id:'sample-message-'+order,order,role:order%2?'assistant':'user',timestamp:now-3600+order*60,text:text[order%text.length]}));
    if(request.order>=0){const message=messages.find(m=>m.order===request.order);if(!message)throw Error('Unknown sample message');return {...base,session,messages:[chunk(message,request.offset)],total:messages.length,has_more:message.order<messages.length-1,next_after:message.order};}
    const page=messages.filter(m=>m.order>request.after).slice(0,5),next=page.at(-1)?.order??request.after;
    return {...base,session,messages:page.map(m=>chunk(m)),total:messages.length,has_more:next<messages.length-1,next_after:next>=0?next:null};
  };
}
