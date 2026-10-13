#pragma once
#include "private_chat_model.h"
class Stream;
void initPrivateChat();
bool requestPrivateSessions(const char*cursor="");
bool requestPrivateMessages(const char*sessionId,int64_t after=-1,int64_t messageOrder=-1,int64_t textOffset=-1);
bool readPrivateChat(privatechat::Model&model,privatechat::State&state);
void clearPrivateChatRuntime();
void setPrivateChatActive(bool active);
bool privateChatConfigured();
void privateChatProvisionTick(Stream&serial);
void privateChatProvisionTick();
int consumePrivateChatViewRequest(); // 0=none,1=open,2=close,3=first local row; USB only.
#ifdef HOMELAB_NATIVE_PREVIEW
uint32_t privateChatGeneration();
bool ingestPrivateChat(const char*json,uint32_t expectedGeneration);
inline bool ingestPrivateChat(const char*json){return ingestPrivateChat(json,privateChatGeneration());} // Synchronous synthetic fixtures only.
void privateChatRuntimeError(const char*genericMessage);
void privateChatRuntimeError(const char*genericMessage,uint32_t expectedGeneration);
extern "C" void homelabPrivateChatRequest(int kind,const char*sessionId,const char*cursor,int64_t after,int64_t messageOrder,int64_t textOffset,uint32_t generation);
#endif
