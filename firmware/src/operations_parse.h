#pragma once
#include "operations_model.h"
#include <ArduinoJson.h>
namespace ops_json {
inline double value(JsonVariantConst v){return v.is<bool>()||v.is<const char*>()?NAN:v.is<double>()?v.as<double>():NAN;}
template<size_t N>inline void text(char(&out)[N],JsonVariantConst v){const char*input=v.is<const char*>()?v.as<const char*>():"";size_t length=strlen(input);if(length>=N){length=N-1;while(length&&(static_cast<unsigned char>(input[length])&0xc0)==0x80)--length;}memcpy(out,input,length);out[length]=0;}
inline bool identity(const char*id){if(!id||!id[0]||strlen(id)>64)return false;for(size_t i=0;id[i];i++){char c=id[i];if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||(i&&(c==':'||c=='.'||c=='_'||c=='-'))))return false;}return true;}
inline bool id(char(&out)[65],JsonVariantConst v){if(!v.is<const char*>()||!identity(v.as<const char*>()))return false;text(out,v);return true;}
inline bool nullableId(char(&out)[65],JsonVariantConst v){if(v.isNull()){out[0]=0;return true;}return id(out,v);}
inline void usage(ops::Usage&u,JsonObjectConst o){u.input=value(o["input_tokens"]);u.output=value(o["output_tokens"]);u.cost=value(o["cost"]);text(u.currency,o["currency"]);text(u.provenance,o["provenance"]);text(u.window,o["window"]);}
inline bool scoped(ops::Identity&r,JsonObjectConst o,const ops::Snapshot&s){if(!id(r.id,o["id"]))return false;int source=ops::sourceIndex(s,o["source_id"]|""),system=ops::systemIndex(s,o["system_id"]|"");if(source<0||system<0)return false;r.source=source;r.system=system;return true;}
template<class T>inline bool duplicate(const T*records,uint8_t count,const T&r){for(int i=0;i<count;i++)if(records[i].system==r.system&&!strcmp(records[i].id,r.id))return true;return false;}
inline bool parse(JsonDocument&doc,ops::Snapshot&s){
  if(strcmp(doc["schema_version"]|"","agent-ops/v1")||!doc["sequence"].is<uint64_t>()||!doc["demo"].is<bool>()||!isfinite(value(doc["generated_at"]))||value(doc["generated_at"])<0)return false;
  const char*arrays[]={"sources","systems","agents","tasks","events","resources","channels"};const uint8_t caps[]={16,8,64,64,64,32,16};for(int i=0;i<7;i++)if(!doc[arrays[i]].is<JsonArrayConst>()||doc[arrays[i]].size()>caps[i])return false;
  new(&s)ops::Snapshot{};s.generated=value(doc["generated_at"]);s.sequence=doc["sequence"].as<uint64_t>();s.demo=doc["demo"]==true;s.received=millis();
  for(JsonObjectConst o:doc["sources"].as<JsonArrayConst>()){
    auto&r=s.sources[s.nSources];if(!id(r.id,o["id"])||ops::sourceIndex(s,r.id)>=0)return false;text(r.name,o["name"]);text(r.status,o["status"]);text(r.provenance,o["provenance"]);r.sampled=value(o["sampled_at"]);r.ttl=value(o["ttl_s"]);if(!isfinite(r.sampled)||r.sampled<0||!isfinite(r.ttl)||r.ttl<1||r.ttl>3600)return false;String capabilities;for(const char*c:o["capabilities"].as<JsonArrayConst>())if(c){if(!strcmp(c,"advisory-heartbeats"))r.advisory=true;if(capabilities.length()+strlen(c)+2<sizeof(r.capabilities)){if(capabilities.length())capabilities+=", ";capabilities+=c;}}strlcpy(r.capabilities,capabilities.c_str(),sizeof(r.capabilities));s.nSources++;
  }
  for(JsonObjectConst o:doc["systems"].as<JsonArrayConst>()){
    auto&r=s.systems[s.nSystems];if(!id(r.id,o["id"])||ops::systemIndex(s,r.id)>=0)return false;int source=ops::sourceIndex(s,o["source_id"]|"");if(source<0)return false;r.source=source;text(r.name,o["name"]);text(r.provider,o["provider"]);text(r.status,o["status"]);r.sampled=value(o["sampled_at"]);r.ttl=value(o["ttl_s"]);auto v=o["summary"];r.total=value(v["agents_total"]);r.recent=value(v["agents_recent"]);r.running=value(v["running"]);r.queued=value(v["queued"]);r.blocked=value(v["blocked"]);r.waiting=value(v["needs_input"]);r.failed=value(v["failed_24h"]);usage(r.usage,o["usage"]);if(!isfinite(r.sampled)||r.sampled<0||!isfinite(r.ttl)||r.ttl<1||r.ttl>3600)return false;s.nSystems++;
  }
  for(JsonObjectConst o:doc["agents"].as<JsonArrayConst>()){
    auto&r=s.agents[s.nAgents];if(!scoped(r,o,s)||duplicate(s.agents,s.nAgents,r)||!nullableId(r.task,o["task_id"]))return false;text(r.name,o["name"]);text(r.provider,o["provider"]);text(r.host,o["host_name"]);text(r.model,o["model"]);text(r.state,o["state"]);r.heartbeat=value(o["heartbeat_at"]);r.ttl=value(o["ttl_s"]);usage(r.usage,o["usage"]);if(!isfinite(r.ttl)||r.ttl<1||r.ttl>3600)return false;s.nAgents++;
  }
  for(JsonObjectConst o:doc["tasks"].as<JsonArrayConst>()){
    auto&r=s.tasks[s.nTasks];if(!scoped(r,o,s)||duplicate(s.tasks,s.nTasks,r)||!nullableId(r.agent,o["agent_id"])||!nullableId(r.parent,o["parent_task_id"]))return false;text(r.title,o["title"]);text(r.state,o["state"]);text(r.project,o["project"]);r.started=value(o["started_at"]);r.updated=value(o["updated_at"]);r.progress=value(o["progress_pct"]);if(isfinite(r.progress)&&(r.progress<0||r.progress>100))return false;s.nTasks++;
  }
  for(JsonObjectConst o:doc["events"].as<JsonArrayConst>()){
    auto&r=s.events[s.nEvents];if(!scoped(r,o,s)||duplicate(s.events,s.nEvents,r)||!nullableId(r.agent,o["agent_id"])||!nullableId(r.task,o["task_id"]))return false;text(r.title,o["title"]);text(r.kind,o["kind"]);text(r.severity,o["severity"]);r.at=value(o["at"]);if(!isfinite(r.at)||r.at<0)return false;s.nEvents++;
  }
  for(JsonObjectConst o:doc["resources"].as<JsonArrayConst>()){
    auto&r=s.resources[s.nResources];if(!scoped(r,o,s)||duplicate(s.resources,s.nResources,r)||!nullableId(r.owner,o["owner_id"]))return false;text(r.name,o["name"]);text(r.state,o["state"]);r.since=value(o["since"]);s.nResources++;
  }
  for(JsonObjectConst o:doc["channels"].as<JsonArrayConst>()){
    auto&r=s.channels[s.nChannels];if(!scoped(r,o,s)||duplicate(s.channels,s.nChannels,r))return false;text(r.name,o["name"]);text(r.status,o["status"]);r.queued=value(o["queued"]);r.unacked=value(o["unacked"]);r.latency=value(o["latency_ms"]);r.sampled=value(o["sampled_at"]);r.ttl=value(o["ttl_s"]);if(!isfinite(r.sampled)||r.sampled<0||!isfinite(r.ttl)||r.ttl<1||r.ttl>3600)return false;s.nChannels++;
  }
  uint8_t sizes[]={s.nSystems,s.nAgents,s.nTasks,s.nEvents,s.nResources,s.nChannels};for(int i=0;i<6;i++){auto count=doc["limits"]["counts"][arrays[i+1]];if(!count.is<uint64_t>())return false;s.counts[i]=count.as<uint64_t>();s.truncated[i]=doc["limits"]["truncated"][arrays[i+1]]==true;if(s.counts[i]<sizes[i]||(s.counts[i]>sizes[i]&&!s.truncated[i]))return false;}
  s.valid=true;return true;
}
}
