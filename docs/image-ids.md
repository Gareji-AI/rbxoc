# Decal IDs versus Image IDs

Roblox image uploads use `assetType = "Decal"`. The completed operation's
`assetId` is the uploaded Decal's ID. An ImageLabel/ImageButton needs the
underlying Image content referenced by `Decal.Texture`; it must not be assumed
to equal that Decal ID. Existing Decal consumers retain the original semantics.

## Supported path: Studio resolution, then explicit offline registration

rbxoc does **not** implement an authenticated Open Cloud Decal.Texture lookup.
Do not assume that metadata, a thumbnail URL, or subtracting one from an ID
resolves an Image ID. No legacy cookie or unsupported endpoint is used.

1. Keep the successful upload's original asset/operation receipt.
2. In Studio's Command Bar, inspect the **existing** Decal. Replace the example
   numeric ID with the actual completed Decal ID:

   ```lua
   local decalId = 123 -- actual completed Decal asset ID
   local container = game:GetService("InsertService"):LoadAsset(decalId)
   local decal = container:FindFirstChildWhichIsA("Decal", true)
   assert(decal, "No Decal found; check asset type, permissions, and moderation")
   print(decal.Texture)
   container:Destroy()
   ```

   `LoadAsset` requires appropriate Studio/account access. If it fails, inspect
   permissions/moderation and retain the original receipt; another upload is
   not a resolution strategy. The unparented container is only inspected, then
   destroyed.

3. Pass the printed Texture to the offline helper, then register both IDs:

   ```bash
   rbxoc asset image-id --decal-id 123 --texture rbxassetid://456
   rbxoc assets register --dir ./art --path ui/icon.png --id 123 --image-id 456
   ```

   The helper also accepts Studio's Roblox asset URL form, for example
   `http://www.roblox.com/asset/?id=456`. Its JSON distinguishes `decalId` and
   `imageId`, and says `resolution = "supplied-studio-texture"` and
   `networkValidated = false`. It parses what **you supplied**; it does not prove
   the association or claim live resolution succeeded.

Without `--texture`, the helper returns `image_resolution_unavailable` with
`imageId = null`, the original `decalId`, and these recovery instructions.
Malformed/untrusted texture formats return `image_texture_invalid`. These
local commands never upload, retry, or discard successful publication receipts.

## Explicit codegen mode

The lockfile's `assets` map still stores the original uploaded ID. A separate
optional `image_ids` table stores the caller-supplied Decal/Image association.
Offline registration remains available without any API credentials or hidden
network validation. Registering an ID does not certify publication freshness.

By default, generated references use the original registered IDs, unchanged
for existing Decal consumers. After resolving/registering all image references,
enable the explicit image-consumer mode:

```toml
[codegen]
output_path = "src/shared/assets"
image_ids = true
```

In this mode image references use the registered Image ID; other asset types
keep their existing IDs. Missing or stale associations fail with an actionable
error before replacing generated output. Both Luau and TypeScript enforce
this rule, although TypeScript emits string types rather than literal IDs.
Replacing the primary Decal ID removes its old association.

For new images, first sync with `image_ids = false`, retain the successful
Decal receipts, resolve/register their Image IDs, then enable image codegen.
With image mode enabled, sync refuses missing image resolution **before**
creating assets; it does not upload and then fail generation. Sync never
silently changes the meaning of an existing `assetId`.

If an older workflow already registered the actual Image ID directly as
`--id`, default codegen remains compatible. The registry does not infer the
type from an arbitrary ID. For explicit image mode, register the known IDs
deliberately, rather than claiming an offline numeric value was verified.

## Live ImageLabel check (not part of offline tests)

In a Studio Play client, create an ImageLabel with the resulting **Image** ID:

```lua
local imageId = 456 -- actual resolved Image ID
local gui = Instance.new("ScreenGui")
gui.Parent = game:GetService("Players").LocalPlayer:WaitForChild("PlayerGui")
local label = Instance.new("ImageLabel")
label.Size = UDim2.fromOffset(128, 128)
label.Image = "rbxassetid://" .. imageId
label.Parent = gui
game:GetService("ContentProvider"):PreloadAsync({label})
print("Image loaded:", label.IsLoaded)
```

Confirm visible content and `IsLoaded = true`, then remove the temporary GUI.
Moderation, access restrictions, or networking can still prevent loading.
The release's fixture tests distinguish IDs and preserve receipts on resolution
failure; they are **not** evidence of a live Studio/ImageLabel verification.
