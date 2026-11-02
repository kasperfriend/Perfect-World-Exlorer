/* Independent x87 instruction reference for 0x41f3d6 / 0x421e80.
 * Run on x86 Linux: cc -O0 tools/check-native-region-reference.c -o /tmp/region-ref
 * Uses the actual FPU at PC24, not a second implementation in Rust/f64.
 * Native D3D8 creation omits FPU_PRESERVE: game flags 1/5 at 0x42fef6;
 * engine bit 8 -> D3D bit 2 at 0x7795c6. This selects PC24, not PC53.
 * This executes recovered arithmetic, NOT the original client/scene runtime.
 */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static float target_delta(float current, float target, float inverse) {
    float out;
    __asm__ volatile("flds %1; fsubs %2; fmuls %3; fstps %0"
        : "=m"(out) : "m"(target), "m"(current), "m"(inverse) : "st");
    return out;
}
static float step(float current, float delta, float seconds) {
    float out;
    __asm__ volatile("flds %1; fmuls %2; fadds %3; fstps %0"
        : "=m"(out) : "m"(delta), "m"(seconds), "m"(current) : "st");
    return out;
}
static float glow_delta(float current, float target, int ms) {
    float out;
    __asm__ volatile("flds %1; fsubs %2; fidivl %3; fstps %0"
        : "=m"(out) : "m"(target), "m"(current), "m"(ms) : "st");
    return out;
}
static float glow_step(float current, float delta, int ms) {
    float out;
    __asm__ volatile("fildl %1; fmuls %2; fadds %3; fstps %0"
        : "=m"(out) : "m"(ms), "m"(delta), "m"(current) : "st");
    return out;
}
static float controller_index(float seconds, float duration) {
    float out, thousand=1000.f, count=127.f;
    __asm__ volatile("flds %1; fmuls %2; fdivs %3; fmuls %4; fstps %0"
        : "=m"(out) : "m"(seconds), "m"(thousand), "m"(duration), "m"(count) : "st");
    return out;
}
static float color_byte(int byte) {
    float out, reciprocal;
    uint32_t u=0x3b808081;
    memcpy(&reciprocal,&u,sizeof u);
    __asm__ volatile("fildl %1; fmuls %2; fstps %0"
        : "=m"(out) : "m"(byte), "m"(reciprocal) : "st");
    return out;
}
static uint32_t bits(float f) {
    uint32_t u;
    memcpy(&u, &f, sizeof u);
    return u;
}
int main(void) {
    unsigned short original, cw;
    __asm__ volatile("fnstcw %0" : "=m"(original));
    /* x87 CW encodings: 00 = 24 significant bits; 10 = 53 bits. */
    cw = original & ~0x300;
    __asm__ volatile("fldcw %0" : : "m"(cw));
    float delta = target_delta(1.f/255.f, 4.f/255.f, .2f);
    float up = .1f, down = .9f;
    float du = target_delta(up, down, .2f);
    float dd = target_delta(down, up, .2f);
    for (int i=0; i<312; ++i) {
        up = step(up, du, .016f);
        down = step(down, dd, .016f);
    }
    uint32_t pc24 = bits(delta), up_word = bits(up), down_word = bits(down);
    uint32_t fade = bits(glow_step(1.f, glow_delta(1.f, 0.f, 1000), 500));
    uint32_t index = bits(controller_index(1.f/127.f,1000.f));
    uint64_t colors=UINT64_C(0xcbf29ce484222325);
    for (int byte=0;byte<256;++byte) {
        uint32_t u=bits(color_byte(byte));
        for (int j=0;j<4;++j) colors=(colors^((u>>(j*8))&255))*UINT64_C(0x100000001b3);
    }
    cw = (original & ~0x300) | 0x200;
    __asm__ volatile("fldcw %0" : : "m"(cw));
    uint32_t pc53 = bits(target_delta(1.f/255.f, 4.f/255.f, .2f));
    __asm__ volatile("fldcw %0" : : "m"(original));
    printf("x87 PC24 delta=%08x up=%08x down=%08x; PC53 delta=%08x\n",
        pc24, up_word, down_word, pc53);
    printf("x87 256 normalized color words FNV=%016llx\n", (unsigned long long)colors);
    printf("x87 PC24 half-time StopFade word=%08x\n", fade);
    printf("x87 PC24 controller sample index word=%08x\n", index);
    return !(index == 0x3f800000 && fade == 0x3f000000 && colors == UINT64_C(0xaf1301c35b66c4a5) && pc24 == 0x3b1a33cf && pc53 == 0x3b1a33ce &&
        up_word == 0x3f6612a9 && down_word == 0x3dcf6ab4);
}
