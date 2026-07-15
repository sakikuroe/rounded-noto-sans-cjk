# Third-Party Notices

This repository's own source code is distributed under the MIT License in
[`LICENSE`](./LICENSE). This file additionally reproduces the license of the
third-party source code whose logic was translated and adapted into this
repository, as required by that license's own terms.

## Resource Han Rounded

The corner-radius formula and the adjacent-radius shrinking logic in
`src/round.rs` (see the doc comments on `corner_radius` and
`compute_cut_plan` for the precise correspondence) are a translation and
adaptation of the corresponding logic in
[Resource Han Rounded](https://github.com/CyanoHao/Resource-Han-Rounded)
(`module/round-font.js`, function `calculateRadius`), which is distributed
under the following license:

```
MIT License

Copyright © 2018—2022 Cyano Hao.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

Note that this notice covers only Resource Han Rounded's *code* (the
license above, quoted from its `LICENSE.md`). It does not cover the fonts
Resource Han Rounded distributes, which are licensed separately under the
SIL Open Font License 1.1 and are unrelated to this repository. See
[`README.md`](./README.md) / [`README.ja.md`](./README.ja.md) for the
licenses that govern the font files this repository's tool produces.
