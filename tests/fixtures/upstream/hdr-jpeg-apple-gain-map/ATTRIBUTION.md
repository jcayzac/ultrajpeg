Source asset: `4955592740.jpg`

Upstream:
- DPReview sample gallery: `iPhone 15 Pro sample gallery`
- Provenance page:
  `https://www.dpreview.com/sample-galleries/9434362346/iphone-15-pro-sample-gallery/4955592740`

Local import note:
- Imported from `/Users/julien.cayzac/4955592740.jpg`

Provenance notes:
- Trusted real-world iPhone 15 HDR JPEG
- Imported into the fixture tree without re-encoding
- Byte-level inspection at import time showed:
  - Apple Gain Map XMP present
  - Apple HDR auxiliary-image marker present
  - MPF container entries present
  - no ISO 21496-1 `APP2` block present

Fixture purpose:
- Real-world trusted HDR JPEG input using Apple Gain Map packaging
- Reference fixture for future Apple Gain Map JPEG input support in [`ultrashiny`](../../../../ultrashiny/),
  gated on upstream `ultrajpeg` support
