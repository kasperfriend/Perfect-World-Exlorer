/* Windows x86 only. Recovered 0x430226 / 0x77b7ed..0x77b859 instructions,
 * with the SAME MSVCRT _CIpow / _ftol exports imported by elementclient.
 * Capture PC24 (D3D8's non-FPU-preserve rendering-thread mode) and PC53
 * separately. This is an arithmetic reference, not original-client execution.
 * Build in an x86 MSVC developer prompt: cl /nologo /W4 /O2 this-file.c
 */
#include <windows.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

static FARPROC native_pow, native_ftol;
static float from_bits(uint32_t bits) {
    float f;
    memcpy(&f, &bits, sizeof f);
    return f;
}
static unsigned short entry(int setting, int index) {
    float c17=from_bits(0x3fd9999a), c30=30.f;
    float c01=from_bits(0x3c23d70a), c255=from_bits(0x3b808081);
    double c65535=65535.;
    int device_gamma, result;
    float exponent;
    int64_t wide;
    __asm {
        fild setting
        fmul c17
        fadd c30
        call native_ftol
        mov device_gamma, eax
    }
    wide=(unsigned int)device_gamma;
    __asm {
        fild wide
        fmul c01
        fld1
        fdiv st(0), st(1)
        fstp exponent
        fstp st(0)
        fild index
        fmul c255
        fld exponent
        call native_pow
        fmul c65535
        call native_ftol
        mov result, eax
    }
    if (index>0 && (unsigned short)result<1) result=1;
    return (unsigned short)result;
}
/* Small immutable numeric reference (51 KiB), also exposed in annotations
 * because some build sandboxes cannot reach GitHub's artifact blob host. */
static unsigned short captured[101*256];
static void emit_pc24_words(unsigned first) {
    const unsigned char *data=(const unsigned char*)captured;
    const char *alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const size_t size=sizeof captured, chunk=3000;
    for (size_t offset=0; offset<size; offset+=chunk) {
        size_t end=offset+chunk<size?offset+chunk:size;
        unsigned part=(unsigned)(offset/chunk+1);
        if(part<first || part>=first+8)continue;
        printf("::notice::PC24-DATA-%u:",(unsigned)(offset/chunk+1));
        for (size_t i=offset;i<end;i+=3) {
            unsigned n=(unsigned)data[i]<<16;
            if(i+1<end)n|=(unsigned)data[i+1]<<8;
            if(i+2<end)n|=data[i+2];
            putchar(alphabet[(n>>18)&63]); putchar(alphabet[(n>>12)&63]);
            putchar(i+1<end?alphabet[(n>>6)&63]:'=');
            putchar(i+2<end?alphabet[n&63]:'=');
        }
        putchar('\n');
    }
}
int main(int argc, char **argv) {
    HMODULE crt=LoadLibraryA("msvcrt.dll");
    unsigned short original,cw,after;
    if ((argc!=2 && argc!=3) || !crt) return 2;
    native_pow=GetProcAddress(crt,"_CIpow");
    native_ftol=GetProcAddress(crt,"_ftol");
    if (!native_pow || !native_ftol) return 3;
    __asm fnstcw original
    for (int mode=0; mode<2; ++mode) {
        char path[2048];
        FILE *file;
        uint64_t hash=UINT64_C(0xcbf29ce484222325);
        sprintf_s(path,sizeof path,"%s-pc%d.bin",argv[1],mode?53:24);
        if (fopen_s(&file,path,"wb") || !file) return 4;
        cw=(original & ~0xf00) | (mode?0x200:0);
        __asm fldcw cw
        for (int setting=0; setting<=100; ++setting) {
            for (int i=0; i<256; ++i) {
                unsigned short word=entry(setting,i);
                if(!mode)captured[setting*256+i]=word;
                fwrite(&word,sizeof word,1,file);
                hash=(hash^(word&255))*UINT64_C(0x100000001b3);
                hash=(hash^(word>>8))*UINT64_C(0x100000001b3);
            }
        }
        __asm fnstcw after
        __asm fldcw original
        fclose(file);
        printf("::notice::MSVCRT x87 PC%d: 25856 words FNV=%016llx CW=%04x after=%04x\n",
            mode?53:24,(unsigned long long)hash,cw,after);
        if ((after&0xf00)!=(cw&0xf00)) return 5;
    }
    emit_pc24_words(argc==3?(unsigned)atoi(argv[2]):1);
    FreeLibrary(crt);
    return 0;
}
