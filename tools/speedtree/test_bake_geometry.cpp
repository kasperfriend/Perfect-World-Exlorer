#include "bake_geometry.hpp"
#include <cassert>
#include <limits>
#include <string>

static uint32_t word(const Writer& writer, size_t offset) {
    uint32_t value;
    std::memcpy(&value, writer.b.data() + offset, sizeof(value));
    return value;
}

int main() {
    const float positions[] = {0,0,0, 1,0,0, 0,1,0, 1,1,0};
    float normals[] = {0,0,1, 0,0,1, 0,0,1, 0,0,1};
    const float uvs[] = {0,0, 1,0, 0,1, 1,1};
    const uint16_t strip[] = {0,1,2,3};
    const uint16_t* strips[] = {strip};
    const uint16_t lengths[] = {4};
    Indexed part{};
    part.count = 4;
    part.coords = positions;
    part.normals = normals;
    part.uv0 = uvs;
    part.nstrips = 1;
    part.lengths = lengths;
    part.strips = strips;
    Writer out;
    indexed(out, part, "branch");
    assert(word(out, 0) == 4);
    assert(word(out, 4 + 4 * 8 * 4) == 6);
    const size_t start = 4 + 4 * 8 * 4 + 4;
    assert(word(out, start) == 0 && word(out, start+4) == 1 && word(out, start+8) == 2);
    assert(word(out, start+12) == 2 && word(out, start+16) == 1 && word(out, start+20) == 3);

    auto expect_failure = [&](const char* component, const char* problem) {
        Writer failed;
        try { indexed(failed, part, component); assert(false && "expected failure"); }
        catch (const std::string& e) {
            assert(e.find(component) != std::string::npos);
            assert(e.find(problem) != std::string::npos);
            assert(failed.b.empty());
        }
    };
    part.normals = nullptr;
    expect_failure("branch", "normal");
    part.normals = normals;
    normals[5] = std::numeric_limits<float>::quiet_NaN();
    expect_failure("frond", "normal");
    normals[5] = 1;
    const uint16_t bad_strip[] = {0,1,9};
    const uint16_t* bad_strips[] = {bad_strip};
    const uint16_t bad_lengths[] = {3};
    part.strips = bad_strips;
    part.lengths = bad_lengths;
    expect_failure("frond", "index");
    Indexed empty{};
    Writer empty_out;
    indexed(empty_out, empty, "branch");
    assert(empty_out.b.size() == 8 && word(empty_out, 0) == 0 && word(empty_out, 4) == 0);
}
