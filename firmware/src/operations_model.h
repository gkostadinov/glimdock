#pragma once
#include <Arduino.h>
#include <math.h>
#include <string.h>
namespace ops {
constexpr uint8_t MAX_SOURCES=16,MAX_SYSTEMS=8,MAX_AGENTS=64,MAX_TASKS=64,MAX_EVENTS=64,MAX_RESOURCES=32,MAX_CHANNELS=16;
struct Usage {double input=NAN,output=NAN,cost=NAN;char currency[4]{},provenance[161]{},window[65]{};};
struct Source {char id[65]{},name[65]{},status[16]{},provenance[161]{},capabilities[257]{};double sampled=NAN,ttl=NAN;bool advisory=false;};
struct Identity {char id[65]{};uint8_t system=255,source=255;};
struct System {char id[65]{},name[65]{},provider[49]{},status[16]{};uint8_t source=255;double sampled=NAN,ttl=NAN,total=NAN,recent=NAN,running=NAN,queued=NAN,blocked=NAN,waiting=NAN,failed=NAN;Usage usage;};
struct Agent:Identity {char name[65]{},provider[49]{},host[65]{},model[65]{},state[16]{},task[65]{};double heartbeat=NAN,ttl=NAN;Usage usage;};
struct Task:Identity {char title[97]{},state[16]{},agent[65]{},parent[65]{},project[65]{};double started=NAN,updated=NAN,progress=NAN;};
struct Event:Identity {char title[97]{},kind[49]{},severity[16]{},agent[65]{},task[65]{};double at=NAN;};
struct Resource:Identity {char name[65]{},state[16]{},owner[65]{};double since=NAN;};
struct Channel:Identity {char name[65]{},status[16]{};double queued=NAN,unacked=NAN,latency=NAN,sampled=NAN,ttl=NAN;};
struct Snapshot {bool valid=false,demo=false;double generated=NAN;uint64_t sequence=0;uint32_t received=0;uint8_t nSources=0,nSystems=0,nAgents=0,nTasks=0,nEvents=0,nResources=0,nChannels=0;Source sources[MAX_SOURCES];System systems[MAX_SYSTEMS];Agent agents[MAX_AGENTS];Task tasks[MAX_TASKS];Event events[MAX_EVENTS];Resource resources[MAX_RESOURCES];Channel channels[MAX_CHANNELS];uint64_t counts[6]{};bool truncated[6]{};};
struct Network {bool connected=false;uint32_t revision=0;char message[97]{"Agent operations unavailable; configure a source in the collector."};};
inline int sourceIndex(const Snapshot&s,const char*id){for(int i=0;i<s.nSources;i++)if(id&&!strcmp(s.sources[i].id,id))return i;return -1;}
inline int systemIndex(const Snapshot&s,const char*id){for(int i=0;i<s.nSystems;i++)if(id&&!strcmp(s.systems[i].id,id))return i;return -1;}
inline int taskIndex(const Snapshot&s,uint8_t sys,const char*id){for(int i=0;i<s.nTasks;i++)if(id&&s.tasks[i].system==sys&&!strcmp(s.tasks[i].id,id))return i;return -1;}
inline int agentIndex(const Snapshot&s,uint8_t sys,const char*id){for(int i=0;i<s.nAgents;i++)if(id&&s.agents[i].system==sys&&!strcmp(s.agents[i].id,id))return i;return -1;}
}
#ifdef HOMELAB_NATIVE_PREVIEW
inline ops::Snapshot&opsTransportSnapshot(){static ops::Snapshot value;return value;}
inline ops::Network&opsTransportState(){static ops::Network value;return value;}
inline bool readOperations(ops::Snapshot&out,ops::Network&state){out=opsTransportSnapshot();state=opsTransportState();return true;}
#ifdef __EMSCRIPTEN__
void requestOperationsTransport();
inline void requestOperationsRefresh(){requestOperationsTransport();}
#else
inline void requestOperationsRefresh(){}
#endif
#else
bool readOperations(ops::Snapshot&out,ops::Network&state);
void requestOperationsRefresh();
#endif
