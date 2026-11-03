// Diagnostic only: compare Compute/LOD counts with the emulated survey.
// Build separately from the baker, for example:
// zig c++ -target x86-windows-gnu -O2 -static -municode tools/speedtree/lod-count-probe.cpp -o lod-count-probe.exe
// Usage: lod-count-probe.exe <SpeedTreeRT.dll> <tree-source directory>
// The directory contains tree-000.spt through tree-058.spt.
#define wmain unused_baker_main
#include "bake_experimental.cpp"
#undef wmain

using Count = unsigned short(METHOD*)(void*);

int wmain(int argc, wchar_t** argv) {
    if (argc != 3) {
        fwprintf(stderr, L"usage: lod-count-probe.exe <SpeedTreeRT.dll> <tree-source directory>\n");
        return 2;
    }
    Api a = open_api(argv[1]);
    auto branch = reinterpret_cast<Count>(symbol(a.dll, "?GetNumBranchLodLevels@CSpeedTreeRT@@QBEGXZ"));
    auto frond = reinterpret_cast<Count>(symbol(a.dll, "?GetNumFrondLodLevels@CSpeedTreeRT@@QBEGXZ"));
    auto leaf = reinterpret_cast<Count>(symbol(a.dll, "?GetNumLeafLodLevels@CSpeedTreeRT@@QBEGXZ"));
    int failures = 0;
    printf("type\tbranch\tfrond\tleaf\tmin_z\tmax_z\n");
    for (unsigned i = 0; i < 59; ++i) {
        wchar_t name[64];
        swprintf(name, 64, L"/tree-%03u.spt", i);
        std::wstring path = std::wstring(argv[2]) + name;
        FILE* f = _wfopen(path.c_str(), L"rb");
        if (!f) { fprintf(stderr, "type %u: cannot open SPT\n", i); ++failures; continue; }
        if (fseek(f, 0, SEEK_END) != 0) { fprintf(stderr, "type %u: cannot seek SPT\n", i); fclose(f); ++failures; continue; }
        long n = ftell(f);
        rewind(f);
        if (n < 1 || n > 100000000) { fprintf(stderr, "type %u: invalid SPT size\n", i); fclose(f); ++failures; continue; }
        std::vector<unsigned char> bytes(static_cast<size_t>(n));
        if (fread(bytes.data(), 1, bytes.size(), f) != bytes.size()) { fprintf(stderr, "type %u: cannot read SPT\n", i); fclose(f); ++failures; continue; }
        fclose(f);
        void* tree = a.alloc(0x9c);
        if (!tree) { fprintf(stderr, "type %u: allocation failed\n", i); ++failures; continue; }
        a.ctor(tree);
        if (!a.load(tree, bytes.data(), static_cast<unsigned>(bytes.size()))) {
            fprintf(stderr, "type %u: LoadTree failed: %s\n", i, a.error());
            ++failures;
        } else {
            float size = 0, variation = 0, bounds[6]{};
            a.getsize(tree, &size, &variation);
            a.setbranch(tree, 0); a.setfrond(tree, 0); a.setleaf(tree, 0);
            a.windb(tree, 2); a.windf(tree, 2); a.windl(tree, 2);
            a.groups(tree, 1);
            a.setsize(tree, size * 0.3048f, 0.f);
            if (!a.compute(tree, nullptr, 1, true)) {
                fprintf(stderr, "type %u: Compute failed: %s\n", i, a.error());
                ++failures;
            } else {
                a.bounds(tree, bounds);
                printf("%u\t%u\t%u\t%u\t%.9g\t%.9g\n", i, branch(tree), frond(tree), leaf(tree), bounds[2], bounds[5]);
            }
        }
        a.dtor(tree);
        a.free(tree);
    }
    fflush(stdout);
    FreeLibrary(a.dll);
    return failures ? 1 : 0;
}
