/* Portable tests use stable IDs and observable data/age, not UI internals. */
#include "node_cache.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>

static void page(GdStatusPage* p, const char* name, const char* value, uint16_t age) {
    memset(p, 0, sizeof(*p)); p->flags = GD_STATUS_VALID; p->age_seconds = age; p->ttl_seconds = 15;
    strcpy(p->title, "System"); strcpy(p->rows[0], name); strcpy(p->rows[1], value);
}
static void catalog(GdNodeCatalog* c, unsigned count, uint32_t generation) {
    memset(c, 0, sizeof(*c)); c->count = (uint8_t)count; c->generation = generation;
    for(unsigned i = 0; i < count; ++i) {
        snprintf(c->nodes[i].id, GD_NODE_ID_BYTES, "node-%u", i);
        page(&c->nodes[i].system, "Duplicate name", c->nodes[i].id, 2);
    }
    if(count) strcpy(c->selected_id, c->nodes[0].id);
}
static size_t packet(const GdNodeCatalog* c, unsigned type, unsigned index, uint32_t seq, uint8_t* wire) {
    const size_t n = gd_catalog_encode(c, (uint8_t)type, (uint8_t)index, 0x80000000u | seq, wire, GD_CACHE_MAX_PACKET);
    assert(n == (type == GD_CATALOG_ENTRY ? 189u : 80u)); return n;
}
static void commit(GdNodeCache* cache, const GdNodeCatalog* c, uint32_t now) {
    uint8_t wire[GD_CACHE_MAX_PACKET];
    assert(gd_node_cache_accept_catalog(cache, wire, packet(c, GD_CATALOG_BEGIN, 0, 1, wire), now) == GdNodeCacheAccepted);
    for(unsigned i = 0; i < c->count; ++i)
        assert(gd_node_cache_accept_catalog(cache, wire, packet(c, GD_CATALOG_ENTRY, i, i + 2, wire), now) == GdNodeCacheAccepted);
    assert(gd_node_cache_accept_catalog(cache, wire, packet(c, GD_CATALOG_COMMIT, 0, 10, wire), now) == GdNodeCacheCommitted);
}
static void make_detail(uint8_t* wire, const char* id, uint32_t seq, uint32_t req, bool complete, const char* value, uint16_t age) {
    GdStatus s; memset(&s, 0, sizeof(s)); s.sequence = seq;
    for(unsigned i = 0; i < GD_STATUS_PAGES; ++i) page(&s.pages[i], id, value, age);
    assert(gd_detail_encode(&s, id, req, complete, wire, GD_DETAIL_BYTES) == GD_DETAIL_BYTES);
}
static void wire_contract(void) {
    GdNodeCatalog* c = (GdNodeCatalog*)calloc(1, sizeof(*c)); assert(c); catalog(c, 8, 0x78563412);
    uint8_t wire[GD_CACHE_MAX_PACKET + 2], original[GD_CACHE_MAX_PACKET]; memset(wire, 0xa5, sizeof(wire));
    size_t n = gd_catalog_encode(c, GD_CATALOG_ENTRY, 7, 0x87654321, wire + 1, sizeof(wire) - 2);
    assert(n == 189 && wire[0] == 0xa5 && wire[190] == 0xa5);
    const uint8_t header[] = {'G','C',2,2,0x21,0x43,0x65,0x87,0x12,0x34,0x56,0x78,8,7,0,0};
    assert(!memcmp(wire + 1, header, sizeof(header)) && !strcmp((char*)wire + 17, "node-7"));
    assert(wire[81] == GD_STATUS_VALID && wire[82] == 2 && wire[84] == 15);
    GdCatalogPacket decoded, before; memset(&decoded, 0xa5, sizeof(decoded)); before = decoded;
    for(size_t i = 0; i < 189; ++i) { assert(!gd_catalog_decode(wire + 1, i, &decoded)); assert(!memcmp(&decoded, &before, sizeof(before))); }
    assert(gd_catalog_decode(wire + 1, n, &decoded) && decoded.index == 7 && decoded.generation == 0x78563412);
    memcpy(original, wire + 1, n);
    const unsigned invalid[] = {0,1,2,3,12,13,14,15,16,80,85};
    for(size_t i = 0; i < sizeof(invalid)/sizeof(invalid[0]); ++i) {
        memcpy(wire, original, n); wire[invalid[i]] = 0xff; decoded = before;
        assert(!gd_catalog_decode(wire, n, &decoded)); assert(!memcmp(&decoded, &before, sizeof(before)));
    }
    memcpy(wire, original, n); wire[6] = 0; wire[7] = 0; assert(!gd_catalog_decode(wire, n, &decoded));
    assert(!gd_catalog_encode(c, GD_CATALOG_ENTRY, 0, 1, wire, sizeof(wire)));
    assert(!gd_catalog_encode(c, GD_CATALOG_BEGIN, 1, 0x80000001, wire, sizeof(wire)));
    assert(!gd_catalog_encode(c, GD_CATALOG_ENTRY, 8, 0x80000001, wire, sizeof(wire)));
    c->count = 9; assert(!gd_catalog_encode(c, GD_CATALOG_BEGIN, 0, 0x80000001, wire, sizeof(wire)));
    c->count = 8;
    make_detail(wire + 1, "node-7", 0x12345678, 0x87654321, true, "full", 4);
    assert(wire[1]=='G' && wire[2]=='V' && wire[3]==2 && wire[4]==1 && wire[5]==0x21 && wire[8]==0x87);
    assert(!strcmp((char*)wire+9,"node-7") && wire[73]=='G' && wire[74]=='D' && wire[77]==0x78);
    uint32_t seq = 0; assert(gd_cache_packet_sequence(wire+1, GD_DETAIL_BYTES, &seq) && seq==0x12345678);
    GdDetail d, dbefore; memset(&d, 0xa5, sizeof(d)); dbefore = d;
    for(size_t i=0;i<GD_DETAIL_BYTES;++i) { assert(!gd_detail_decode(wire+1,i,&d)); assert(!memcmp(&d,&dbefore,sizeof(d))); }
    assert(gd_detail_decode(wire+1,GD_DETAIL_BYTES,&d) && d.complete && d.request_sequence==0x87654321);
    memcpy(original,wire+1,GD_DETAIL_BYTES);
    for(unsigned i=0;i<2;++i) {
        memcpy(wire,original,GD_DETAIL_BYTES); gd_cache_write_u32(wire+76,i ? 0x80000001 : 0);
        d=dbefore; assert(!gd_detail_decode(wire,GD_DETAIL_BYTES,&d)); assert(!memcmp(&d,&dbefore,sizeof(d)));
    }
    memcpy(wire,original,GD_DETAIL_BYTES); wire[15]=1; assert(!gd_detail_decode(wire,GD_DETAIL_BYTES,&d)); /* noncanonical ID padding */
    GdStatus s=d.status; s.sequence=0x80000000; assert(!gd_detail_encode(&s,"node",1,true,wire,sizeof(wire)));
    char id[GD_NODE_ID_BYTES], old_id[GD_NODE_ID_BYTES]; memset(id,0xa5,sizeof(id)); memcpy(old_id,id,sizeof(id));
    memset(wire,0xa5,sizeof(wire));
    assert(gd_cache_control_encode(0x78563412,"node-7",wire+1,sizeof(wire)-2)==70);
    const uint8_t gs[]={'G','S',0x12,0x34,0x56,0x78}; assert(!memcmp(wire+1,gs,sizeof(gs)) && wire[0]==0xa5 && wire[71]==0xa5);
    seq=0; assert(gd_cache_control_decode(wire+1,70,&seq,id,sizeof(id)) && seq==0x78563412 && !strcmp(id,"node-7"));
    for(size_t i=0;i<70;++i) { seq=123; memcpy(id,old_id,sizeof(id)); assert(!gd_cache_control_decode(wire+1,i,&seq,id,sizeof(id))); assert(seq==123 && !memcmp(id,old_id,sizeof(id))); }
    assert(!gd_cache_control_encode(0,"node",wire,sizeof(wire)));
    assert(!gd_cache_control_encode(1,"bad/id",wire,sizeof(wire)));
    char long_id[65]; memset(long_id,'a',64); long_id[64]=0; assert(!gd_cache_control_encode(1,long_id,wire,sizeof(wire)));
    long_id[63]=0; assert(gd_cache_control_encode(1,long_id,wire,sizeof(wire))==70);
    seq=123; memcpy(id,old_id,sizeof(id)); assert(!gd_cache_control_decode(wire,70,&seq,id,63)); assert(seq==123 && !memcmp(id,old_id,sizeof(id)));
    free(c);
}
static void atomic_catalog_and_identity(void) {
    GdNodeCache* cache=(GdNodeCache*)calloc(1,sizeof(*cache)); GdNodeCatalog* c=(GdNodeCatalog*)calloc(1,sizeof(*c)); assert(cache&&c);
    uint8_t wire[GD_CACHE_MAX_PACKET]; GdStatus out; uint32_t req; char id[64];
    catalog(c,8,1); strcpy(c->selected_id,"node-3");
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,1,0,1,wire),1000)==GdNodeCacheAccepted);
    for(unsigned i=0;i<8;++i) assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,i,i+2,wire),2000+i*100)==GdNodeCacheAccepted);
    assert(!gd_node_cache_project(cache,&out,5000));
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,3,0,10,wire),5000)==GdNodeCacheCommitted);
    assert(!strcmp(gd_node_cache_current_id(cache),"node-3") && gd_node_cache_selection(cache,&req,id) && req==1);
    assert(gd_node_cache_project(cache,&out,5000) && out.pages[0].age_seconds==4);
    assert(!strcmp(out.pages[1].rows[0],"Duplicate name") && !strcmp(out.pages[1].rows[1],"Details not loaded"));
    make_detail(wire,"node-3",1,req,true,"node three full",3);
    assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),5000) && !gd_node_cache_selected_loading(cache));
    assert(gd_node_cache_project(cache,&out,6000) && !strcmp(out.pages[1].rows[1],"node three full") && out.pages[1].age_seconds==4);
    /* Same full wire after reset must not make a disconnected source young again. */
    gd_node_cache_reset_transport(cache); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),20000));
    assert(gd_node_cache_project(cache,&out,20000) && out.pages[1].age_seconds==18 && (out.pages[1].flags&GD_STATUS_STALE));
    /* Reverse order and duplicate display names preserve stable-ID selection and full detail. */
    catalog(c,8,2); for(unsigned i=0;i<8;++i) c->nodes[i].system.age_seconds=3; for(unsigned i=0;i<4;++i) { GdCatalogNode temp=c->nodes[i]; c->nodes[i]=c->nodes[7-i]; c->nodes[7-i]=temp; }
    commit(cache,c,21000); assert(!strcmp(gd_node_cache_current_id(cache),"node-3"));
    assert(gd_node_cache_project(cache,&out,21000) && !strcmp(out.pages[1].rows[1],"node three full") && out.pages[1].age_seconds==19);
    /* Identical committed packet retransmits do not renew summary staging age. */
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,4,6,wire),30000)==GdNodeCacheAccepted);
    assert(gd_node_cache_project(cache,&out,30000) && out.pages[0].age_seconds==12);
    /* Catalog summary can be newer than the full System while cached detail retains its age. */
    assert(!strcmp(out.pages[0].rows[1],"node-3") && !strcmp(out.pages[1].rows[1],"node three full"));
    catalog(c,2,3); strcpy(c->selected_id,"node-1"); commit(cache,c,31000);
    assert(!strcmp(gd_node_cache_current_id(cache),"node-1") && gd_node_cache_selection(cache,&req,id) && req==2);
    assert(gd_node_cache_project(cache,&out,31000) && !strcmp(out.pages[1].rows[1],"Details not loaded"));
    catalog(c,0,4); commit(cache,c,32000); assert(!gd_node_cache_project(cache,&out,32000) && !gd_node_cache_selected_loading(cache));
    free(cache); free(c);
}
static void malformed_transactions(void) {
    GdNodeCache* cache=(GdNodeCache*)calloc(1,sizeof(*cache)); GdNodeCatalog* c=(GdNodeCatalog*)calloc(1,sizeof(*c)); assert(cache&&c);
    uint8_t wire[GD_CACHE_MAX_PACKET]; GdStatus out; catalog(c,2,10); commit(cache,c,1000);
    catalog(c,2,11); c->nodes[0].system.age_seconds=3; c->nodes[1].system.age_seconds=3; assert(gd_node_cache_accept_catalog(cache,wire,packet(c,1,0,11,wire),2000)==GdNodeCacheAccepted);
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,3,0,20,wire),2000)==GdNodeCacheRejected); /* missing entries */
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,0,12,wire),2100)==GdNodeCacheAccepted);
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,0,12,wire),9000)==GdNodeCacheAccepted); /* exact retry */
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,0,13,wire),9000)==GdNodeCacheRejected);
    strcpy(c->nodes[1].id,"node-0"); assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,1,13,wire),9000)==GdNodeCacheRejected);
    strcpy(c->nodes[1].id,"node-1"); c->generation=12; assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,1,13,wire),9000)==GdNodeCacheRejected);
    c->generation=11; c->count=1; assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,0,13,wire),9000)==GdNodeCacheRejected);
    c->count=2; assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,1,13,wire),2200)==GdNodeCacheAccepted);
    strcpy(c->selected_id,"node-1"); assert(gd_node_cache_accept_catalog(cache,wire,packet(c,3,0,20,wire),9000)==GdNodeCacheRejected);
    strcpy(c->selected_id,"node-0"); assert(gd_node_cache_accept_catalog(cache,wire,packet(c,3,0,20,wire),9000)==GdNodeCacheCommitted);
    assert(gd_node_cache_project(cache,&out,10000) && out.pages[0].age_seconds==10); /* uses 2100 receipt, not 9000 retry/commit */
    /* A new generation is an authoritative source-age snapshot; unlike
     * retransmission, its entry receipt establishes a new baseline. */
    c->generation=12; commit(cache,c,12000);
    assert(gd_node_cache_project(cache,&out,13000) && out.pages[0].age_seconds==4);
    c->generation=10; assert(gd_node_cache_accept_catalog(cache,wire,packet(c,1,0,21,wire),10000)==GdNodeCacheRejected);
    free(cache); free(c);
}
static void fast_navigation_and_replays(void) {
    GdNodeCache* cache=(GdNodeCache*)calloc(1,sizeof(*cache)); GdNodeCatalog* c=(GdNodeCatalog*)calloc(1,sizeof(*c)); assert(cache&&c);
    uint8_t wire[GD_DETAIL_BYTES], saved[GD_DETAIL_BYTES]; uint32_t req; char id[64]; GdStatus out;
    catalog(c,2,1); memset(c->nodes[0].system.rows[0],0,GD_STATUS_TEXT); memset(c->nodes[1].system.rows[0],0,GD_STATUS_TEXT); strcpy(c->nodes[0].system.rows[0],"Alpha"); strcpy(c->nodes[1].system.rows[0],"Beta"); commit(cache,c,1000);
    assert(gd_node_cache_selection(cache,&req,id) && req==1 && !strcmp(id,"node-0"));
    make_detail(wire,"node-0",1,1,true,"original alpha",1); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),1000));
    assert(gd_node_cache_select_step(cache,1,2000,&req,id) && req==2 && !strcmp(id,"node-1"));
    assert(gd_node_cache_project(cache,&out,2000) && !strcmp(out.pages[0].rows[0],"Beta") && !strcmp(out.pages[1].rows[0],"Beta"));
    assert(gd_node_cache_select_step(cache,1,2100,&req,id) && req==3 && !strcmp(id,"node-0"));
    assert(gd_node_cache_selected_loading(cache));
    /* Delayed response from previous Alpha visit cannot satisfy current request. */
    make_detail(wire,"node-0",2,1,true,"obsolete alpha",2); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2200));
    assert(gd_node_cache_selected_loading(cache) && !strcmp(gd_node_cache_current_id(cache),"node-0"));
    make_detail(wire,"node-1",3,2,true,"background beta",2); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2300));
    assert(gd_node_cache_selected_loading(cache) && !strcmp(gd_node_cache_current_id(cache),"node-0"));
    make_detail(wire,"unknown",4,3,true,"unknown",0); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2400));
    make_detail(wire,"node-0",5,3,true,"current alpha",1); memcpy(saved,wire,sizeof(wire)); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2500));
    assert(!gd_node_cache_selected_loading(cache));
    make_detail(wire,"node-0",6,1,true,"late old alpha",0); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2600));
    make_detail(wire,"node-0",7,3,false,"loading placeholders",0); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2700));
    assert(gd_node_cache_project(cache,&out,3500) && !strcmp(out.pages[1].rows[1],"current alpha") && out.pages[1].age_seconds==2);
    assert(gd_node_cache_accept_detail(cache,saved,sizeof(saved),10000));
    assert(gd_node_cache_project(cache,&out,10000) && out.pages[1].age_seconds==8);
    /* Older valid wire is ACK/drop, retaining receipt age and current data. */
    make_detail(wire,"node-0",4,3,true,"replay",0); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),11000));
    assert(gd_node_cache_project(cache,&out,11000) && !strcmp(out.pages[1].rows[1],"current alpha") && out.pages[1].age_seconds==9);
    assert(gd_node_cache_select_step(cache,-1,12000,&req,id) && !strcmp(id,"node-1"));
    assert(gd_node_cache_project(cache,&out,12000) && !strcmp(out.pages[1].rows[1],"background beta"));
    gd_node_cache_reset_transport(cache); /* Offline choice survives a fresh transport session. */
    catalog(c,2,1); commit(cache,c,13000); assert(!strcmp(gd_node_cache_current_id(cache),"node-1"));
    assert(gd_node_cache_selection(cache,&req,id) && req==4 && !strcmp(id,"node-1"));
    free(cache); free(c);
}
static void rollover_and_freshness(void) {
    GdNodeCache* cache=(GdNodeCache*)calloc(1,sizeof(*cache)); GdNodeCatalog* c=(GdNodeCatalog*)calloc(1,sizeof(*c)); assert(cache&&c);
    uint8_t wire[GD_DETAIL_BYTES]; GdStatus out; uint32_t req; char id[64];
    catalog(c,2,UINT32_MAX); commit(cache,c,UINT32_MAX-999u);
    assert(gd_node_cache_project(cache,&out,1000) && out.pages[0].age_seconds==4);
    make_detail(wire,"node-0",0x7fffffff,1,true,"pre wrap",1); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),1000));
    make_detail(wire,"node-0",1,1,true,"post wrap",1); assert(gd_node_cache_accept_detail(cache,wire,sizeof(wire),2000));
    assert(gd_node_cache_project(cache,&out,2000) && !strcmp(out.pages[1].rows[1],"post wrap"));
    catalog(c,2,1); c->nodes[0].system.age_seconds=1; c->nodes[0].system.flags|=GD_STATUS_STALE; commit(cache,c,2000);
    assert(gd_node_cache_project(cache,&out,2000) && !strcmp(out.pages[0].rows[1],"post wrap")); /* Explicitly stale summary cannot outrank fresh detail. */
    cache->request_sequence=UINT32_MAX; assert(gd_node_cache_select_step(cache,1,2000,&req,id) && req==1);
    c->generation=2; c->nodes[1].system.age_seconds=GD_STATUS_UNKNOWN_AGE; c->nodes[1].system.flags|=GD_STATUS_DEMO; commit(cache,c,3000);
    assert(gd_node_cache_project(cache,&out,4000) && out.pages[0].age_seconds==GD_STATUS_UNKNOWN_AGE && (out.pages[0].flags&GD_STATUS_STALE));
    assert((out.pages[1].flags&GD_STATUS_DEMO) && !strcmp(out.pages[1].rows[1],"Details not loaded"));
    c->generation=3; c->nodes[1].system.age_seconds=65530; commit(cache,c,4000);
    assert(gd_node_cache_project(cache,&out,14000) && out.pages[0].age_seconds==65534 && (out.pages[0].flags&GD_STATUS_STALE));
    free(cache); free(c);
}
static void fresh_unchanged_catalogs(void) {
    GdNodeCache* cache=(GdNodeCache*)calloc(1,sizeof(*cache)); GdNodeCatalog* c=(GdNodeCatalog*)calloc(1,sizeof(*c)); assert(cache&&c);
    GdStatus out; uint8_t wire[GD_CACHE_MAX_PACKET]; catalog(c,1,1); c->nodes[0].system.age_seconds=0;
    for(unsigned generation=1;generation<=4;++generation) {
        c->generation=generation; commit(cache,c,generation*10000u);
        assert(gd_node_cache_project(cache,&out,generation*10000u+5000u));
        assert(out.pages[0].age_seconds==5 && !(out.pages[0].flags&GD_STATUS_STALE));
    }
    /* A retransmitted entry retains its prior receipt; source age keeps running. */
    assert(gd_node_cache_accept_catalog(cache,wire,packet(c,2,0,2,wire),60000)==GdNodeCacheAccepted);
    assert(gd_node_cache_project(cache,&out,60000) && out.pages[0].age_seconds==20 && (out.pages[0].flags&GD_STATUS_STALE));
    /* A new catalog with an honestly old source also remains stale immediately. */
    c->generation=5; c->nodes[0].system.age_seconds=25; commit(cache,c,61000);
    assert(gd_node_cache_project(cache,&out,61000) && out.pages[0].age_seconds==25 && (out.pages[0].flags&GD_STATUS_STALE));
    free(cache); free(c);
}
int main(void) {
    wire_contract(); atomic_catalog_and_identity(); malformed_transactions(); fast_navigation_and_replays(); rollover_and_freshness(); fresh_unchanged_catalogs();
    puts("Flipper cache protocol, atomic catalogs, instant stable-ID navigation, replay and offline freshness checks passed."); return 0;
}
