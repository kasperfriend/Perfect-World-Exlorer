# Graphics technology notices

This repository's project license remains [GPL-2.0-or-later](LICENSE). These
notices apply **only** to the separately credited material below. They do not
relicense the rest of the project.

## AMD FidelityFX Super Resolution 1

`src/fsr1.wgsl` adapts the EASU and RCAS equations from AMD's
[`ffx_fsr1.h`](https://github.com/GPUOpen-Effects/FidelityFX-FSR/blob/master/ffx-fsr/ffx_fsr1.h).
The port uses explicit texture loads instead of gather4 and guards flat/endpoint
reciprocals. It does not include or execute the AMD FSR temporal/ML SDK or its
frame-generation components. The following is the copyright and license notice
from the [FSR 1 distribution](https://github.com/GPUOpen-Effects/FidelityFX-FSR/blob/master/license.txt):

> Copyright (c) 2021 Advanced Micro Devices, Inc. All rights reserved.
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

## SDKs not included

The proprietary NVIDIA DLSS feature plugins, Intel XeSS SDK binaries and AMD
FSR SDK binaries/sources are **not bundled, linked, loaded, or called** by this
project. Their names in the Graphics menu identify disabled, *unavailable*
choices, not included products. NVIDIA's open Streamline framework has its own
license and separate binary plugins. AMD's newer FidelityFX SDK uses
file-specific terms; its main SDK license contains binary-only redistribution
and no-reverse-engineering restrictions. XeSS uses the Intel Simplified Software
License for binary-only software. None of these terms can be made GPL-compatible
by editing this project's GPL text. Review the exact components and obtain
appropriate permissions and legal advice before incorporating or distributing
any of them. See [GRAPHICS_TECHNOLOGIES.md](GRAPHICS_TECHNOLOGIES.md).
