- Connect to Titanium login servers without eagerly hashing the installation;
  check requested files when the world asks and show cancellable file-check
  progress. On every server, P99 included, files whose size and modification
  time are unchanged are answered from the networking layer's cache; a refusal
  makes the next login reread the refused files. The cache cannot see a
  same-size edit with a restored modification time. Delete
  `asset-checksums-v1.json` from the settings directory to clear it by hand.
