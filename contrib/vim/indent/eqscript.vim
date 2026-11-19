" Indentation for the EQ client scenario language. License: MIT.
" A line after a header ending in ':' is indented one level; elif and else
" line up with their if; break, continue and pass end a level.
if exists('b:did_indent')
  finish
endif
let b:did_indent = 1
setlocal autoindent indentexpr=EqscriptIndent() indentkeys+=0=elif,0=else
let b:undo_indent = 'setlocal autoindent< indentexpr< indentkeys<'

if exists('*EqscriptIndent')
  finish
endif

" The code of a line, without its comment; '#' inside quotes is text.
function! s:Code(lnum) abort
  return substitute(getline(a:lnum),
        \ '\v^(([^"''#]|"([^"\\]|\\.)*"|''([^''\\]|\\.)*'')*)#.*$', '\1', '')
endfunction

function! EqscriptIndent() abort
  if getline(v:lnum) =~# '^\s*\%(elif\|else\)\>'
    let current = indent(v:lnum)
    let lnum = v:lnum - 1
    while lnum > 0
      if getline(lnum) =~# '^\s*\%(if\|elif\)\>' && indent(lnum) <= current
        return indent(lnum)
      endif
      let lnum -= 1
    endwhile
    return current
  endif
  let previous = prevnonblank(v:lnum - 1)
  while previous > 0 && getline(previous) =~# '^\s*#'
    let previous = prevnonblank(previous - 1)
  endwhile
  if previous == 0
    return 0
  endif
  let code = s:Code(previous)
  if code =~# ':\s*$'
    return indent(previous) + shiftwidth()
  endif
  if code =~# '^\s*\%(break\|continue\|pass\)\s*$'
    return max([indent(previous) - shiftwidth(), 0])
  endif
  return indent(previous)
endfunction
