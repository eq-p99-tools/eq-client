" Vim/Neovim syntax for the EQ client scenario language. License: MIT.
if exists('b:current_syntax')
  finish
endif

syntax case match
syntax match eqscriptCommand /^\s*\zs\%(login\|wait_servers\|server\|wait_select\|select\|create\|wait_online\|wait_zone\|wait_dead\|slash\|gm\|chat\|drag\|press\|hold\|wait\|camera\|trace\|face\|move\|approach\|chase\|walk\|give\|interact\|inspect_link\|click\|right_click\|hover\|hold_click\|report\|sound\|screenshot\|quit\)\>/
syntax match eqscriptNumber /\(^\|\s\)\zs[-+]\=\d\+\%(\.\d\+\)\=\%([eE][-+]\=\d\+\)\=\ze\($\|\s\|#\)/
syntax match eqscriptKey /\c\<\%(ctrl\|alt\|shift\|[a-z0-9]\|f\d\{1,2}\|space\|escape\|esc\|enter\|return\|backspace\|delete\|tab\|slash\|up\|down\|left\|right\|numlock\|insert\|home\|end\|pageup\|pagedown\|numpad[0-9]\|numpad_\%(add\|subtract\|multiply\|divide\|decimal\|enter\)\)\%(+\%(ctrl\|alt\|shift\|[a-z0-9]\|f\d\{1,2}\|space\|escape\|esc\|enter\|tab\|up\|down\|left\|right\)\)*\>/ contained
syntax match eqscriptCommand /^\s*\zs\%(press\|hold\)\>/ nextgroup=eqscriptKey skipwhite
syntax match eqscriptComment /#.*/ contains=eqscriptTodo,@Spell
syntax keyword eqscriptTodo TODO FIXME NOTE XXX contained

highlight default link eqscriptCommand Statement
highlight default link eqscriptNumber Number
highlight default link eqscriptKey Special
highlight default link eqscriptComment Comment
highlight default link eqscriptTodo Todo

let b:current_syntax = 'eqscript'
