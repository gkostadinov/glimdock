#include "network_recovery.h"
#include <algorithm>
#include <cassert>
#include <cstdio>
#include <cstring>
#include <string>
using glimdock::BodyRead;
struct Clock { uint32_t base=0, elapsed=0; uint32_t now() const{return base+elapsed;} };
struct Client {
    Clock& clock;std::string text;size_t offset=0;uint32_t interval=0,closeAt=UINT32_MAX;unsigned reads=0;
    int available(){size_t visible=interval?std::min(text.size(),size_t(clock.elapsed/interval)+1):text.size();return int(visible-offset);}
    bool connected(){return clock.elapsed<closeAt;}
    int read(uint8_t* out,size_t n){++reads;n=std::min(n,size_t(available()));memcpy(out,text.data()+offset,n);offset+=n;return int(n);}
};
struct Sink { std::string text;size_t capacity=4096;size_t write(const uint8_t* p,size_t n){if(n>capacity-text.size())return 0;text.append((const char*)p,n);return n;} };
BodyRead read(Client& c,Sink& s,int n,size_t limit=4096){return glimdock::readBoundedBody(c,s,n,limit,5000,2000,[&]{return c.clock.now();},[&]{c.clock.elapsed+=5;});}
int main(){
    {Clock t;Client c{t,std::string(1000,'x')};Sink s;assert(read(c,s,1000)==BodyRead::Complete&&s.text==c.text);}
    // Actual hang regression: socket remains connected after a partial body.
    {Clock t;Client c{t,"partial"};Sink s;assert(read(c,s,100)==BodyRead::Timeout&&t.elapsed<=2010&&s.text=="partial");}
    // Activity cannot postpone the total deadline indefinitely.
    {Clock t;Client c{t,std::string(100,'x')};c.interval=1800;Sink s;assert(read(c,s,100)==BodyRead::Timeout&&t.elapsed==5000);}
    {Clock t;Client c{t,"truncated"};c.closeAt=10;Sink s;assert(read(c,s,100)==BodyRead::Incomplete);}
    {Clock t;Client c{t,"x"};Sink s;assert(read(c,s,100,10)==BodyRead::TooLarge&&c.reads==0);}
    {Clock t;Client c{t,"close delimited"};c.closeAt=10;Sink s;assert(read(c,s,-1)==BodyRead::Complete&&s.text==c.text);}
    {Clock t;Client c{t,"oversized"};Sink s;assert(read(c,s,-1,4)==BodyRead::TooLarge&&s.text.size()==4);}
    {Clock t;Client c{t,""};Sink s;assert(read(c,s,-1)==BodyRead::Timeout);}
    {Clock t;t.base=UINT32_MAX-100;Client c{t,"partial"};Sink s;assert(read(c,s,99)==BodyRead::Timeout&&t.elapsed<=2010);}
    {Clock t;Client c{t,"capacity"};Sink s;s.capacity=2;assert(read(c,s,8)==BodyRead::TooLarge&&s.text.empty());}
    // Leave any following response bytes unread after the declared body.
    {Clock t;Client c{t,"{}extra"};Sink s;assert(read(c,s,2)==BodyRead::Complete&&s.text=="{}"&&c.offset==2);}
    {
        glimdock::NetworkRecovery p;p.reset(0,true);
        const uint32_t hours=6*60*60*1000;
        assert(!p.reconnect(hours,true));assert(!p.reconnect(hours,false));
        assert(!p.reconnect(hours+29999,false));assert(p.reconnect(hours+30000,false));
        assert(!p.reconnect(hours+30001,false));assert(!p.reconnect(hours+89999,false));
        assert(p.reconnect(hours+90000,false));assert(p.reconnect(hours+210000,false));
        assert(!p.reconnect(hours+210001,false));
        assert(!p.reconnect(hours+220000,true));assert(!p.reconnect(hours+220001,false));
    }
    {
        glimdock::NetworkRecovery p;p.reset(0,true);
        for(uint32_t t=0;t<120000;t+=3000){p.failed(t);assert(!p.reconnect(t,true));}
        assert(p.reconnect(120000,true));assert(!p.reconnect(120001,true));
        for(uint32_t t=123000;t<240000;t+=3000){p.failed(t);assert(!p.reconnect(t,true));}
        assert(p.reconnect(240000,true));
        p.failed(250000);p.healthy();assert(!p.reconnect(500000,true)&&p.failures()==0);
    }
    {glimdock::NetworkRecovery p;const uint32_t start=UINT32_MAX-15000;p.reset(start,false);assert(!p.reconnect(start+29999,false));assert(p.reconnect(start+30000,false));}
    std::puts("Passed: stalled/partial/dripping/oversized HTTP, wraparound deadlines, Wi-Fi grace/backoff and healthy-link recovery.");
}
