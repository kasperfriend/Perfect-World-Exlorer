// Diagnostic: compare per-leaf normals at rest with 64 sequential wind samples.
// Diagnostic only: run one SPT per process because SpeedTreeRT has global state.
// zig c++ -target x86-windows-gnu -O2 -static -municode tools/speedtree/leaf-normal-wind-probe.cpp -o leaf-normal-wind-probe.exe
// Usage: leaf-normal-wind-probe.exe <SpeedTreeRT.dll> <one SPT file>
#define wmain unused_baker_main
#include "bake.cpp"
#undef wmain
using Count = unsigned short(METHOD*)(void*);

int wmain(int argc, wchar_t** argv) {
    if (argc != 3) { fprintf(stderr, "expected DLL and SPT\n"); return 2; }
    FILE* f = _wfopen(argv[2], L"rb");
    if (!f || fseek(f, 0, SEEK_END)) { fprintf(stderr, "cannot read SPT\n"); return 1; }
    long n=ftell(f); rewind(f);
    if (n < 1 || n > 100000000) { fclose(f); return 1; }
    std::vector<unsigned char> bytes(static_cast<size_t>(n));
    if (fread(bytes.data(),1,bytes.size(),f) != bytes.size()) { fclose(f); return 1; }
    fclose(f);
    Api a=open_api(argv[1]);
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
    unsigned counts[]={0,0,leaf(tree)};
    printf("lod\tleaves\tchanged_components\ttested_components\tmax_abs_difference\n");
    for (unsigned idx=0;idx<counts[2];++idx) {
        a.lod(tree,1.f-(float(idx)+0.5f)/float(counts[2]));
        a.rock(tree,false); a.reset(tree); a.clock(0); a.strength(tree,0,-1,-1);
        Geometry g{}; a.geo(tree,&g,4,-1,-1,idx);
        if (g.leaves[0].count && !g.leaves[0].normals) return 3;
        std::vector<float> baseline;
        if (g.leaves[0].count) baseline.assign(g.leaves[0].normals,g.leaves[0].normals + 3*g.leaves[0].count);
        unsigned changed=0, total=0; double maxdiff=0;
        a.rock(tree,true);
        for (unsigned t=0;t<64;++t) {
            a.clock(t*0.25f); a.strength(tree,1.f,-1,-1); Geometry v{}; a.geo(tree,&v,4,-1,-1,idx);
            if (v.leaves[0].count != baseline.size()/3 || (v.leaves[0].count && !v.leaves[0].normals)) return 3;
            for (unsigned j=0;j<baseline.size();++j) {
                float x=v.leaves[0].normals[j];
                if (!std::isfinite(x) || !std::isfinite(baseline[j])) return 3;
                float diff=fabsf(x-baseline[j]);
                if (diff>0) ++changed;
                if (diff>maxdiff) maxdiff=diff;
                ++total;
            }
        }
        printf("%u\t%zu\t%u\t%u\t%.9g\n",idx,baseline.size()/3,changed,total,maxdiff);
    }
    unsigned mismatches=0;
    fflush(stdout); a.dtor(tree); a.free(tree); FreeLibrary(a.dll); return mismatches ? 1 : 0;
}
