// Compare explicitly requested component geometry with automatic LOD geometry.
// Diagnostic only: run one SPT per process because SpeedTreeRT has global state.
// zig c++ -target x86-windows-gnu -O2 -static -municode tools/speedtree/lod-explicit-probe.cpp -o lod-explicit-probe.exe
// Usage: lod-explicit-probe.exe <SpeedTreeRT.dll> <one SPT file> [--combinations]
// In combinations mode, every explicit branch/frond/leaf index combination
// is compared to the corresponding individually requested component.
#define wmain unused_baker_main
#include "bake_experimental.cpp"
#undef wmain
using Count = unsigned short(METHOD*)(void*);

static void hash_bytes(uint64_t& hash, const void* p, size_t n) {
    if (n && !p) throw "missing geometry array";
    auto bytes = static_cast<const unsigned char*>(p);
    for (size_t i = 0; i < n; ++i) hash = (hash ^ bytes[i]) * UINT64_C(1099511628211);
}
static uint64_t hash_indexed(const Indexed& part) {
    uint64_t h = UINT64_C(14695981039346656037);
    hash_bytes(h, &part.count, sizeof(part.count));
    hash_bytes(h, &part.nstrips, sizeof(part.nstrips));
    hash_bytes(h, part.coords, size_t(part.count)*3*sizeof(float));
    hash_bytes(h, part.normals, size_t(part.count)*3*sizeof(float));
    hash_bytes(h, part.uv0, size_t(part.count)*2*sizeof(float));
    hash_bytes(h, part.lengths, size_t(part.nstrips)*sizeof(uint16_t));
    if (part.nstrips && !part.strips) throw "missing strips";
    for (unsigned i = 0; i < part.nstrips; ++i)
        hash_bytes(h, part.strips[i], size_t(part.lengths[i])*sizeof(uint16_t));
    return h;
}
static uint64_t hash_leaf(const Leaf& leaf) {
    uint64_t h = UINT64_C(14695981039346656037);
    hash_bytes(h, &leaf.count, sizeof(leaf.count));
    hash_bytes(h, leaf.centers, size_t(leaf.count)*3*sizeof(float));
    hash_bytes(h, leaf.normals, size_t(leaf.count)*3*sizeof(float));
    hash_bytes(h, leaf.clusters, leaf.count);
    if (leaf.count && (!leaf.texcoords || !leaf.mapcoords)) throw "missing leaf coordinate arrays";
    for (unsigned i = 0; i < leaf.count; ++i) {
        hash_bytes(h, leaf.texcoords[i], 8*sizeof(float));
        hash_bytes(h, leaf.mapcoords[i], 16*sizeof(float));
    }
    return h;
}
int wmain(int argc, wchar_t** argv) {
    bool combinations = argc == 4 && wcscmp(argv[3], L"--combinations") == 0;
    if (argc != 3 && !combinations) { fprintf(stderr, "expected DLL and SPT [--combinations]\n"); return 2; }
    FILE* f = _wfopen(argv[2], L"rb");
    if (!f || fseek(f, 0, SEEK_END)) { fprintf(stderr, "cannot read SPT\n"); return 1; }
    long n=ftell(f); rewind(f);
    if (n < 1 || n > 100000000) { fclose(f); return 1; }
    std::vector<unsigned char> bytes(static_cast<size_t>(n));
    if (fread(bytes.data(),1,bytes.size(),f) != bytes.size()) { fclose(f); return 1; }
    fclose(f);
    Api a=open_api(argv[1]);
    auto branch=reinterpret_cast<Count>(symbol(a.dll,"?GetNumBranchLodLevels@CSpeedTreeRT@@QBEGXZ"));
    auto frond=reinterpret_cast<Count>(symbol(a.dll,"?GetNumFrondLodLevels@CSpeedTreeRT@@QBEGXZ"));
    auto leaf=reinterpret_cast<Count>(symbol(a.dll,"?GetNumLeafLodLevels@CSpeedTreeRT@@QBEGXZ"));
    void* tree=a.alloc(0x9c); if (!tree) return 1; a.ctor(tree);
    if (!a.load(tree,bytes.data(),static_cast<unsigned>(bytes.size()))) return 1;
    float size=0,variation=0; a.getsize(tree,&size,&variation);
    a.setbranch(tree,0); a.setfrond(tree,0); a.setleaf(tree,0);
    a.windb(tree,2); a.windf(tree,2); a.windl(tree,2); a.groups(tree,1);
    a.setsize(tree,size*0.3048f,0.f);
    if (!a.compute(tree,nullptr,1,true)) return 1;
    using Drop=void(*)(bool);
    reinterpret_cast<Drop>(symbol(a.dll,"?SetDropToBillboard@CSpeedTreeRT@@SAX_N@Z"))(false);
    float eye[]={0.f,0.f,0.f},dir[]={1.f,0.f,0.f}; a.camera(eye,dir);
    a.rock(tree,false); a.reset(tree); a.clock(0.f); a.strength(tree,0.f,-1.f,-1.f);
    unsigned counts[]={branch(tree),frond(tree),leaf(tree)};
    std::vector<uint64_t> hashes[3];
    if (!combinations) printf("component\trequested\tauto_lod\texplicit_lod\tauto_count\texplicit_count\tauto_hash\texplicit_hash\n");
    for (unsigned component=0; component<3; ++component) for (unsigned i=0; i<counts[component]; ++i) {
        // The midpoint avoids discrete or leaf-transition boundaries.
        float level=1.f-(float(i)+0.5f)/float(counts[component]);
        a.lod(tree,level); Geometry automatic{};
        a.geo(tree,&automatic,1u<<component,-1,-1,-1);
        int auto_lod=component==0?automatic.branches.lod:component==1?automatic.fronds.lod:automatic.leaves[0].lod;
        unsigned auto_count=component==0?automatic.branches.count:component==1?automatic.fronds.count:automatic.leaves[0].count;
        uint64_t auto_hash=component==0?hash_indexed(automatic.branches):component==1?hash_indexed(automatic.fronds):hash_leaf(automatic.leaves[0]);
        // Request the same index while the continuous level selects the other end.
        a.lod(tree,level>0.5f?0.f:1.f); Geometry explicit_geo{};
        a.geo(tree,&explicit_geo,1u<<component,component==0?i:-1,component==1?i:-1,component==2?i:-1);
        int explicit_lod=component==0?explicit_geo.branches.lod:component==1?explicit_geo.fronds.lod:explicit_geo.leaves[0].lod;
        unsigned explicit_count=component==0?explicit_geo.branches.count:component==1?explicit_geo.fronds.count:explicit_geo.leaves[0].count;
        uint64_t explicit_hash=component==0?hash_indexed(explicit_geo.branches):component==1?hash_indexed(explicit_geo.fronds):hash_leaf(explicit_geo.leaves[0]);
        hashes[component].push_back(explicit_hash);
        if (!combinations) printf("%u\t%u\t%d\t%d\t%u\t%u\t%016llx\t%016llx\n",component,i,auto_lod,explicit_lod,auto_count,explicit_count,(unsigned long long)auto_hash,(unsigned long long)explicit_hash);
    }
    unsigned cases=0, mismatches=0;
    if (combinations) {
        for (unsigned b=0;b<counts[0];++b) for (unsigned f=0;f<counts[1];++f) for (unsigned l=0;l<counts[2];++l) {
            a.lod(tree,0.37f); Geometry g{}; a.geo(tree,&g,7,b,f,l);
            ++cases;
            if (g.branches.lod!=int(b) || g.fronds.lod!=int(f) || g.leaves[0].lod!=int(l) ||
                hash_indexed(g.branches)!=hashes[0][b] || hash_indexed(g.fronds)!=hashes[1][f] || hash_leaf(g.leaves[0])!=hashes[2][l]) {
                fprintf(stderr,"combination %u %u %u mismatched\n",b,f,l); ++mismatches;
            }
        }
        printf("%u\t%u\n",cases,mismatches);
    }
    fflush(stdout); a.dtor(tree); a.free(tree); FreeLibrary(a.dll); return mismatches ? 1 : 0;
}
