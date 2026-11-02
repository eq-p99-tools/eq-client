" Detect only the dedicated extension; do not claim generic .txt/.script files.
augroup eqscript_filetype
  autocmd!
  autocmd BufRead,BufNewFile *.eqscript setfiletype eqscript
augroup END
