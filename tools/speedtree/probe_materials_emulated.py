#!/usr/bin/env python3
"""Experimental SpeedTreeRT SPT material probe, NOT a Windows oracle.

Example (dependencies in a venv or installed into .cache/py):
  PYTHONPATH=.cache/py python3 tools/speedtree/probe_materials_emulated.py \
    google_drive/world.ecwld google_drive/trees.pck google_drive/speedtreert.dll --type 0

Executes the client DLL under Speakeasy/Unicorn with substitute CRT routines,
including hand-written x87 code and intentionally fake file-descriptor APIs.
By default only LoadTree and material getters are exercised. Optional
--compute-lod-counts also calls Compute, whose results are unvalidated and
may be affected by emulated CRT, floating point, and random-number behavior.
No geometry, lighting, rendering, alpha, billboard, or wind results are claimed. The raw
13 floats returned by each getter have not yet been mapped to renderer state.
Do not use these measurements as production material constants without an
independent Windows/client comparison.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

import pefile
import speakeasy

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pck_lib
from asset_survey import tree_types
from probe_lod_unicorn import DLL_SHA256

# _ftol pops ST(0) using truncate-to-zero and returns the signed 64-bit
# integer in EDX:EAX. The other x87 code implements asin/acos and positive
# base pow; these are substitutes, not MSVCRT implementation bytes.
FTOL = bytes.fromhex('83ec0cd97c24080fb74424080d000c0000668944240ad96c240adf3c24d96c24088b04248b54240483c40cc3')
TRIG = bytes.fromhex('d9c0d8c8d9e8dee1d9fad9c9d9f3c3d9c0d8c8d9e8dee1d9fad9f3c3')
# Zero base with positive exponent is used by the LOD transition path.
# Negative bases, zero/negative exponents and NaNs remain unsupported.
POW = bytes.fromhex('d9eedbeaddd87507ddd8ddd8d9eec3d9c9d9f1d9c0d9fcdce9d9c9d9f0d9e8dec1d9fdddd9c3')


def run(world, archive, dll, type_id, compute_lod_counts=False, max_api_count=10000):
    if hashlib.sha256(dll.read_bytes()).hexdigest() != DLL_SHA256:
        raise ValueError('unexpected DLL hash')
    types = tree_types(world.read_bytes())
    if not 0 <= type_id < len(types):
        raise ValueError(f'tree type {type_id} out of range')
    path = types[type_id][0]
    _, entries = pck_lib.directory(archive)
    matches = [e for e in entries if e[0].lower() == path.lower()]
    if len(matches) != 1:
        raise ValueError(f'type {type_id}: expected exactly one SPT record for {path}')
    data = pck_lib.read_entry(archive, matches[0])
    if len(data) != matches[0][2]:
        raise ValueError(f'type {type_id}: invalid SPT length')

    em = speakeasy.Speakeasy()
    mod = em.load_module(path=str(dll))
    em.emu.max_api_count = max_api_count
    em.run_module(mod)
    if em.emu.runs[-1].error:
        raise RuntimeError(f'DLL entry failed: {em.emu.runs[-1].error}')

    # Speakeasy 1.5.11's C++ allocation stub has an incorrect argc; get the
    # argument from the cdecl stack rather than the empty hook argument list.
    em.add_api_hook(lambda e, api, orig, argv: em.mem_alloc(max(e.get_func_argv(0, 1)[0], 1)),
                    'msvcrt', '??2@YAPAXI@Z', 1, 0)
    em.add_api_hook(lambda e, api, orig, argv: 0xffffffff, 'msvcrt', '_get_osfhandle', 1, 0)
    em.add_api_hook(lambda e, api, orig, argv: 0xffffffff, 'msvcrt', '_fstat', 2, 0)
    em.add_api_hook(lambda e, api, orig, argv: int(argv[0] in (9, 10, 11, 12, 13, 32)),
                    'msvcrt', 'isspace', 1, 0)

    def scan(e, api, orig, argv):
        args = e.get_func_argv(0, 3)
        text = e.read_mem_string(args[0], 1)
        fmt = e.read_mem_string(args[1], 1)
        if fmt != '%s':
            raise ValueError(f'unsupported sscanf format {fmt!r}')
        words = text.split()
        if not words:
            return 0xffffffff
        e.mem_write(args[2], words[0].encode('ascii') + b'\0')
        return 1
    em.add_api_hook(scan, 'msvcrt', 'sscanf', 0, 0)

    stub = em.mem_alloc(0x1000)
    em.mem_write(stub, FTOL)
    em.mem_write(stub + 0x300, TRIG)
    em.mem_write(stub + 0x400, POW)
    # atof returns a double on ST(0). Compute the token in Python then execute
    # FLD qword ptr [buffer]; RET in the emulator. This is not a general CRT.
    em.mem_write(stub + 0x100, b'\xdd\x05' + struct.pack('<I', stub + 0x200) + b'\xc3')
    pe = pefile.PE(str(dll))
    patches = {b'_ftol': stub, b'atof': stub + 0x100, b'_CIacos': stub + 0x300,
               b'_CIasin': stub + 0x30f, b'_CIpow': stub + 0x400}
    for desc in pe.DIRECTORY_ENTRY_IMPORT:
        for imp in desc.imports:
            if imp.name in patches:
                em.mem_write(imp.address, struct.pack('<I', patches[imp.name]))

    def atof_hook(e, address, size, ctx):
        ptr = e.get_func_argv(0, 1)[0]
        text = e.read_mem_string(ptr, 1)
        value = float(text)
        if not math.isfinite(value):
            raise ValueError(f'nonfinite atof input {text!r}')
        e.mem_write(stub + 0x200, struct.pack('<d', value))
    em.add_code_hook(atof_hook, begin=stub + 0x100, end=stub + 0x100)

    # The emulator's _initterm implementation is a no-op. Run this build's
    # initializer table explicitly, in order; fail on every emulation error.
    for (fn,) in struct.iter_unpack('<I', bytes(em.mem_read(0x10062000, 0x278))):
        if fn:
            em.call(fn)
            if em.emu.runs[-1].error:
                raise RuntimeError(f'initializer {fn:#x}: {em.emu.runs[-1].error}')

    obj = em.mem_alloc(0x9c)
    source = em.mem_alloc(len(data))
    em.mem_write(source, data)
    exports = {e.name.decode(): 0x10000000 + e.address for e in mod.DIRECTORY_ENTRY_EXPORT.symbols if e.name}

    def call(name, args=()):
        em.reg_write('ecx', obj)
        em.call(exports[name], list(args))
        error = em.emu.runs[-1].error
        if error:
            raise RuntimeError(f'{name}: {error}')
        return em.reg_read('eax')

    call('??0CSpeedTreeRT@@QAE@XZ')
    if not (call('?LoadTree@CSpeedTreeRT@@QAE_NPBEI@Z', [source, len(data)]) & 0xff):
        raise RuntimeError(f'type {type_id}: LoadTree returned false')
    materials = {}
    for kind in ('Branch', 'Frond', 'Leaf'):
        ptr = call(f'?Get{kind}Material@CSpeedTreeRT@@QBEPBMXZ')
        if not ptr:
            raise RuntimeError(f'type {type_id}: null {kind} material')
        values = struct.unpack('<13f', em.mem_read(ptr, 52))
        if not all(map(math.isfinite, values)):
            raise RuntimeError(f'type {type_id}: nonfinite {kind} material')
        materials[kind.lower()] = values
    # ParseLodInfo writes the mode and three floats into these object slots.
    # +0x28 and +0x44 are also read by GetTransitionValues callers. The raw
    # offsets are retained until their precise semantics are independently
    # validated; LoadTree alone does not establish the generated LOD counts.
    lod_mode = struct.unpack('<I', em.mem_read(obj + 0x18, 4))[0]
    lod_values = struct.unpack('<4f', em.mem_read(obj + 0x1c, 16))
    alpha_reference = bytes(em.mem_read(obj + 0x44, 1))[0]
    if not all(map(math.isfinite, lod_values)):
        raise RuntimeError(f'type {type_id}: nonfinite LOD parameter')
    geometry_probe = None
    if compute_lod_counts:
        # Mirror the baker's settings; this is not verified native geometry.
        def bits(value):
            return struct.unpack('<I', struct.pack('<f', value))[0]
        size_ptr = em.mem_alloc(8)
        call('?GetTreeSize@CSpeedTreeRT@@QBEXAAM0@Z', [size_ptr, size_ptr + 4])
        size, variation = struct.unpack('<ff', em.mem_read(size_ptr, 8))
        for kind in ('Branch', 'Leaf', 'Frond'):
            call(f'?Set{kind}LightingMethod@CSpeedTreeRT@@QAEXW4ELightingMethod@1@@Z', [0])
            call(f'?Set{kind}WindMethod@CSpeedTreeRT@@QAEXW4EWindMethod@1@@Z', [2])
        call('?SetNumLeafRockingGroups@CSpeedTreeRT@@QAEXI@Z', [1])
        call('?SetTreeSize@CSpeedTreeRT@@QAEXMM@Z', [bits(size * .3048), bits(0)])
        if not (call('?Compute@CSpeedTreeRT@@QAE_NPBMI_N@Z', [0, 1, 1]) & 255):
            error_ptr = call('?GetCurrentError@CSpeedTreeRT@@SAPBDXZ')
            message = em.emu.read_mem_string(error_ptr, 1) if error_ptr else '(no DLL error)'
            raise RuntimeError(f'type {type_id}: Compute returned false: {message}')
        counts = {kind.lower(): call(f'?GetNum{kind}LodLevels@CSpeedTreeRT@@QBEGXZ') & 65535
                  for kind in ('Branch', 'Frond', 'Leaf')}
        geometry_probe = {'source_size': size, 'source_variation': variation,
                          'counts': counts}
    return {'type_id': type_id, 'spt_path': path,
            'spt_sha256': hashlib.sha256(data).hexdigest(), 'materials': materials,
            **({'experimental_compute': geometry_probe} if geometry_probe is not None else {}),
            'lod_raw': {'mode_18': lod_mode, 'floats_1c_20_24_28': lod_values,
                        'byte_44': alpha_reference}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('world', type=Path)
    parser.add_argument('trees', type=Path)
    parser.add_argument('dll', type=Path)
    parser.add_argument('--type', required=True, type=int)
    parser.add_argument('--compute-lod-counts', action='store_true')
    parser.add_argument('--max-api-count', type=int, default=10000,
                        help='Speakeasy API-call budget per run (default: 10000)')
    args = parser.parse_args()
    if args.max_api_count < 1:
        parser.error('--max-api-count must be positive')
    print(json.dumps(run(args.world, args.trees, args.dll, args.type, args.compute_lod_counts,
                         args.max_api_count), ensure_ascii=False))


if __name__ == '__main__':
    main()
