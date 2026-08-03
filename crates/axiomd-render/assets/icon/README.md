# Callout icons

The Lucide subset a callout is drawn with — one icon per kind `callout.rs` knows,
plus the arrow a foldable callout's `<details>` marker is replaced by. Nothing
here is fetched: the files are compiled into the binary, served to the document
from the app's own `axiomd://assets/icon/` path, and written into an exported
file as `data:` bytes.

Only used icons are bundled, and
`callout::tests::every_kind_has_a_bundled_icon_and_no_icon_is_unused` holds that
line in both directions — an icon nothing references fails the suite.

Source: <https://lucide.dev> (`lucide-icons/lucide`, `icons/<name>.svg`), taken
verbatim apart from having their attributes collapsed onto one line. `LICENSE`
beside them is the project's own, unedited: ISC for Lucide, and MIT for the
icons it derives from Feather — of the ones bundled here, `check`,
`chevron-right`, `info`, `x`, `triangle-alert` (Feather's `alert-triangle`) and
`circle-question-mark` (Feather's `help-circle`).
