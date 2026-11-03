// Runtime indexed geometry serialization shared by the baker and its tests.
// This format is PWTREE07; it does not yet store tangents or material state.
#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

struct Indexed {
    int lod;
    uint16_t nstrips;
    const uint16_t* lengths;
    const uint16_t* const* strips;
    uint16_t count;
    const uint32_t* colors;
    const float *normals, *binormals, *tangents, *coords, *uv0, *uv1, *weights;
    const uint8_t* wind;
};

struct Writer {
    std::vector<uint8_t> b;
    void raw(const void* p, size_t n) {
        if (n) {
            if (!p) throw std::string("null geometry pointer");
            auto q = static_cast<const uint8_t*>(p);
            b.insert(b.end(), q, q + n);
        }
    }
    void u(uint32_t x) { raw(&x, 4); }
    void f(float x) {
        if (!std::isfinite(x)) throw std::string("nonfinite geometry");
        raw(&x, 4);
    }
    void text(const char* p) {
        size_t n = p ? strlen(p) : 0;
        if (n > 4096) throw std::string("invalid texture name");
        u(static_cast<uint32_t>(n));
        raw(p, n);
    }
};

// Fail instead of inventing missing or invalid normals: the file name is
// reported by bake(), and the component/material is included in each error.
static void indexed(Writer& w, const Indexed& s, const char* material) {
    auto fail = [material](const char* why) { throw std::string(material) + " material: " + why; };
    const unsigned n = s.count;
    if (n && (!s.coords || !s.uv0)) fail("missing vertex positions or UVs");
    if (n && !s.normals) fail("missing vertex normals; no fallback is supported");
    for (unsigned i = 0; i < n; ++i) {
        for (int j = 0; j < 3; ++j) {
            if (!std::isfinite(s.coords[3*i+j])) fail("nonfinite vertex position");
            if (!std::isfinite(s.normals[3*i+j]))
                throw std::string(material) + " material: nonfinite vertex normal at vertex " +
                    std::to_string(i) + ", component " + std::to_string(j) + "; no fallback is supported";
        }
        for (int j = 0; j < 2; ++j)
            if (!std::isfinite(s.uv0[2*i+j])) fail("nonfinite vertex UV");
    }
    std::vector<uint32_t> indices;
    for (unsigned k = 0; k < s.nstrips; ++k) {
        if (!s.lengths || !s.strips || !s.strips[k]) fail("missing triangle strip");
        for (unsigned j = 2; j < s.lengths[k]; ++j) {
            uint32_t a = s.strips[k][j-2], b = s.strips[k][j-1], c = s.strips[k][j];
            if (a >= n || b >= n || c >= n) fail("triangle index out of range");
            if (a == b || b == c || a == c) continue;
            if (j & 1) std::swap(a, b);
            indices.push_back(a); indices.push_back(b); indices.push_back(c);
        }
    }
    w.u(n);
    for (unsigned i = 0; i < n; ++i) {
        for (int j = 0; j < 3; ++j) w.f(s.coords[3*i+j]);
        for (int j = 0; j < 3; ++j) w.f(s.normals[3*i+j]);
        for (int j = 0; j < 2; ++j) w.f(s.uv0[2*i+j]);
    }
    w.u(static_cast<uint32_t>(indices.size()));
    w.raw(indices.data(), indices.size() * sizeof(uint32_t));
}
