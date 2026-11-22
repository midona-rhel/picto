# AI Tagging

[← User Guide](README.md)

Picto can suggest tags for images with AI models that run on your computer. Picto never uploads
media for this. Only images are analyzed. For a collection, Picto analyzes the members that are
images.

## Download a model

Open **Settings → AI Models**. Each model is listed with the data it was trained on, the size of
its download and a reference speed.

| Model | Trained on | Tags | Size |
|---|---|---|---|
| WD14 SWN (SwinV2 v3) | Danbooru | general, character, rating | 467 MB |
| Z3D (E621 ConvNext) | e621 | general, creator, series, character, species | 390 MB |
| WD14 EVA (EVA02-Large v3) | Danbooru | general, character, rating. This model has the highest accuracy. | 1.26 GB |
| OppaiOracle (V1.1) | Danbooru | general tags from a wide vocabulary | 993 MB |

Click **Download** next to a model. **Cancel** stops a download, and **Delete** removes a
downloaded model. **Model storage** shows the disk space that the models use.

On macOS a downloaded model can show an **Optimize** button. It prepares a Core ML version of the
model, which tags faster on that Mac.

## Tag selected items

1. Select one or more images.
2. Press `Mod+Shift+A`, right-click the selection and choose **Auto Tag**, or click the AI button
   in the inspector, whose tooltip is "Suggest tags with AI".
3. In the panel, choose one or more downloaded models in the panel's sidebar and click **Run**.
   Picto remembers which models you chose.
4. Review the suggestions. `←` and `→`, or the thumbnails, go from one item to the next.
5. Click the **Apply** button, which shows the number of tags it will apply.

The panel opens in the middle of the window when you use the key or the menu. It opens beside the
inspector when you use the inspector's button.

The review works like this:

- **Suggested** lists the tags at or above the **Minimum confidence** slider, and **Below cutoff**
  lists the others. The slider starts at the confidence level of the General group (see below). It
  applies to every tag group, and only in this run.
- The tags above the cutoff start checked. Click a tag to check or uncheck it.
- The **Filter suggestions...** field searches the list.

Applying tags can be undone with `Mod+Z`.

## Auto-tag new imports

**Settings → AI Models → Behavior** has two switches:

- **Auto-tag new imports** runs the selected models after an import and applies every tag above
  the confidence levels. It needs at least one downloaded model. Choose under the switch which
  models run.
- **Write rating tags** also stores the predicted rating (general, sensitive, questionable or
  explicit) as a `rating:` tag. It is on by default.

## Confidence levels

**Settings → AI Models → Confidence** sets the default cutoff for each tag group:

| Group | Default |
|---|---|
| General | 35% |
| Character | 85% |
| Species | 35% |
| Series | 85% |
| Creator | 85% |
| Rating | 50% |

Auto-tagging of new imports uses these levels.
