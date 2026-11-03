// Trace component LOD selection and alpha-reference values from the client DLL.
// zig c++ -target x86-windows-gnu -O2 -static -municode tools/speedtree/lod-geometry-probe.cpp -o lod-geometry-probe.exe
// Usage: lod-geometry-probe.exe <SpeedTreeRT.dll> <one SPT file> <steps>
#define wmain unused_baker_main
#include "bake.cpp"
#undef wmain

int wmain(int argc, wchar_t** argv) {
    if (argc != 4) {
        fwprintf(stderr, L"usage: lod-geometry-probe.exe <SpeedTreeRT.dll> <one SPT file> <steps>\n");
        return 2;
    }
    int steps = _wtoi(argv[3]);
    if (steps < 1 || steps > 10000) { fprintf(stderr, "steps must be 1..10000\n"); return 2; }
    FILE* f = _wfopen(argv[2], L"rb");
    if (!f) { fprintf(stderr, "cannot open SPT\n"); return 1; }
    if (fseek(f, 0, SEEK_END)) { fclose(f); return 1; }
    long n = ftell(f); rewind(f);
    if (n < 1 || n > 100000000) { fclose(f); return 1; }
    std::vector<unsigned char> bytes(static_cast<size_t>(n));
    if (fread(bytes.data(), 1, bytes.size(), f) != bytes.size()) { fclose(f); return 1; }
    fclose(f);
    Api a = open_api(argv[1]);
    void* tree = a.alloc(0x9c);
    if (!tree) { fprintf(stderr, "allocation failed\n"); return 1; }
    a.ctor(tree);
    if (!a.load(tree, bytes.data(), static_cast<unsigned>(bytes.size()))) {
        fprintf(stderr, "LoadTree failed: %s\n", a.error()); return 1;
    }
    float size = 0, variation = 0;
    a.getsize(tree, &size, &variation);
    a.setbranch(tree, 0); a.setfrond(tree, 0); a.setleaf(tree, 0);
    a.windb(tree, 2); a.windf(tree, 2); a.windl(tree, 2);
    a.groups(tree, 1); a.setsize(tree, size * 0.3048f, 0.f);
    if (!a.compute(tree, nullptr, 1, true)) { fprintf(stderr, "Compute failed: %s\n", a.error()); return 1; }
    // The client initializes this global setting to false.
    using Drop = void(*)(bool);
    reinterpret_cast<Drop>(symbol(a.dll, "?SetDropToBillboard@CSpeedTreeRT@@SAX_N@Z"))(false);
    float eye[] = {0.f, 0.f, 0.f}, direction[] = {1.f, 0.f, 0.f};
    a.camera(eye, direction);
    printf("level\tbranch_lod\tbranch_alpha\tbranch_count\tfrond_lod\tfrond_alpha\tfrond_count\tleaf0_active\tleaf0_lod\tleaf0_alpha\tleaf0_count\tleaf1_active\tleaf1_lod\tleaf1_alpha\tleaf1_count\n");
    for (int i = 0; i <= steps; ++i) {
        float level = static_cast<float>(i) / static_cast<float>(steps);
        a.lod(tree, level);
        Geometry g{};
        a.geo(tree, &g, 7, -1, -1, -1);
        printf("%.9g\t%d\t%.9g\t%u\t%d\t%.9g\t%u\t%d\t%d\t%.9g\t%u\t%d\t%d\t%.9g\t%u\n",
               level, g.branches.lod, g.alpha_branch, g.branches.count,
               g.fronds.lod, g.alpha_frond, g.fronds.count,
               g.leaves[0].active, g.leaves[0].lod, g.leaves[0].alpha, g.leaves[0].count,
               g.leaves[1].active, g.leaves[1].lod, g.leaves[1].alpha, g.leaves[1].count);
    }
    fflush(stdout);
    a.dtor(tree); a.free(tree); FreeLibrary(a.dll);
    return 0;
}
