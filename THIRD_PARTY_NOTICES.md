# Third-party notices

MixCanvas includes the following components in addition to the dependencies
listed by Cargo and pnpm.

## Beat This! and beat-this-rs

The beat/downbeat models and their Rust inference integration are derived from
Beat This! and beat-this-rs, distributed under the MIT License.

Copyright (c) 2025 danigb (Rust port)  
Copyright (c) 2024 Institute of Computational Perception, JKU Linz, Austria
("Beat This!" original work)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Bundled model files:

- `beat_this_small.onnx` — SHA-256
  `a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f`
- `mel_spectrogram.onnx` — SHA-256
  `fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9`

Sources:

- https://github.com/CPJKU/beat_this
- https://github.com/danigb/beat-this-rs

## Open-Unmix

The vocal separation model is derived from Open-Unmix (UMX-HQ), distributed
under the MIT License on the terms reproduced above.

Copyright (c) 2019 Inria and the Open-Unmix contributors

Bundled model file:

- `open-unmix-vocals-fp16.onnx` — SHA-256
  `a1ed651a83f3b0ba39b728f11e877ec586c86b51b6ecb25a26d7c2878cfaf496`

Source: https://github.com/sigsep/open-unmix-pytorch

## ONNX Runtime

The models above are executed by ONNX Runtime, whose Windows binaries are
bundled with the program. It is distributed under the MIT License on the terms
reproduced above.

Copyright (c) Microsoft Corporation

Bundled binaries:

- `onnxruntime.dll` — SHA-256
  `e7eedec6a6f26dc39dc948276a75ef6d2bee3fff944d874ceed0bbd3b97bff40`
- `onnxruntime_providers_shared.dll` — SHA-256
  `265c8daf29637cb259cac8be9f08f2cd45f3883f0f0e4949cbfddd5b4cbec3b6`

Source: https://github.com/microsoft/onnxruntime
