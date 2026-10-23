## Default Permission

Default permissions for the local-files plugin.

Deliberately empty of `allow-*`: every command is invoked from Rust through
`run_mobile_plugin`, which does not consult the ACL. Nothing in the frontend
addresses `plugin:local-files|…` directly — it goes through the app's own
`local_*` commands, which run under `core:default` — so granting these to the
webview would widen the surface for no gain.

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`local-files:allow-cancel-enumerate`

</td>
<td>

Enables the cancel_enumerate command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-cancel-enumerate`

</td>
<td>

Denies the cancel_enumerate command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-create-document`

</td>
<td>

Enables the create_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-create-document`

</td>
<td>

Denies the create_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-delete-document`

</td>
<td>

Enables the delete_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-delete-document`

</td>
<td>

Denies the delete_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-document-exists`

</td>
<td>

Enables the document_exists command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-document-exists`

</td>
<td>

Denies the document_exists command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-enumerate-tree`

</td>
<td>

Enables the enumerate_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-enumerate-tree`

</td>
<td>

Denies the enumerate_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-find-document`

</td>
<td>

Enables the find_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-find-document`

</td>
<td>

Denies the find_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-list-persisted`

</td>
<td>

Enables the list_persisted command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-list-persisted`

</td>
<td>

Denies the list_persisted command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-open-fd`

</td>
<td>

Enables the open_fd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-open-fd`

</td>
<td>

Denies the open_fd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-open-write-fd`

</td>
<td>

Enables the open_write_fd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-open-write-fd`

</td>
<td>

Denies the open_write_fd command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-pick-files`

</td>
<td>

Enables the pick_files command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-pick-files`

</td>
<td>

Denies the pick_files command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-pick-text-document`

</td>
<td>

Enables the pick_text_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-pick-text-document`

</td>
<td>

Denies the pick_text_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-pick-tree`

</td>
<td>

Enables the pick_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-pick-tree`

</td>
<td>

Denies the pick_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-pick-writable-tree`

</td>
<td>

Enables the pick_writable_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-pick-writable-tree`

</td>
<td>

Denies the pick_writable_tree command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-read-bytes`

</td>
<td>

Enables the read_bytes command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-read-bytes`

</td>
<td>

Denies the read_bytes command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-release-uri`

</td>
<td>

Enables the release_uri command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-release-uri`

</td>
<td>

Denies the release_uri command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:allow-rename-document`

</td>
<td>

Enables the rename_document command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`local-files:deny-rename-document`

</td>
<td>

Denies the rename_document command without any pre-configured scope.

</td>
</tr>
</table>
