# AI Tagging

[← User Guide](README.md)

Picto can suggest tags for images with AI models that run on your computer. Media is never
uploaded. Only images are analyzed; for a collection, its image members are used.

## Download a model

Open **Settings → AI Models**. Each model shows its training data, download size and a reference
speed.

| Model | Trained on | Tags | Size |
|---|---|---|---|
| WD14 SWN (SwinV2 v3) | Danbooru | general, character, rating | 467 MB |
| Z3D (E621 ConvNext) | e621 | general, creator, series, character, species | 390 MB |
| WD14 EVA (EVA02-Large v3) | Danbooru | general, character, rating; highest accuracy | 1.26 GB |
| OppaiOracle (V1.1) | Danbooru | wide-vocabulary general tags | 993 MB |

Click **Download** next to a model. **Cancel** stops a download; **Delete** removes a downloaded
model. **Model storage** shows the disk space used.

On macOS, a downloaded model may show **Optimize**. This prepares a Core ML version for faster
tagging on that Mac.

## Tag selected items

1. Select one or more images.
2. Press `Mod+Shift+A`, right-click → **Auto Tag**, or click the AI button in the inspector
   ("Suggest tags with AI").
3. In the panel, choose one or more downloaded models in the sidebar and click **Run**. Picto
   remembers your model choice.
4. Review the suggestions. Use `←`/`→` (or the thumbnails) to move between items.
5. Click **Apply N tags**.

How review works:

- **Suggested** lists tags at or above the **Minimum confidence** slider; **Below cutoff** lists
  the rest. The slider starts at the General confidence level (below) and applies to every tag
  group in this run only.
- Tags above the cutoff start checked. Click a tag to check or uncheck it.
- **Filter suggestions…** searches the list.

Applying tags can be undone with `Mod+Z`.

## Auto-tag new imports

In **Settings → AI Models → Behavior**:

- **Auto-tag new imports** — after an import, run the selected models and apply every tag above the
  confidence levels. Needs at least one downloaded model; choose which models run underneath the
  switch.
- **Write rating tags** — also store the predicted rating (general, sensitive, questionable or
  explicit) as a `rating:` tag. On by default.

## Confidence levels

**Settings → AI Models → Confidence** sets the default cutoff per tag group:

| Group | Default |
|---|---|
| General | 35% |
| Character | 85% |
| Species | 35% |
| Series | 85% |
| Creator | 85% |
| Rating | 50% |

Auto-tagging on import uses these levels.
