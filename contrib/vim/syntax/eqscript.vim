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
syntax keyword eqscriptFlowKeyword limit contained
syntax keyword eqscriptType int bool text contained
syntax keyword eqscriptValue true false known stale contained
syntax match eqscriptField /\<\%(client\|zone\|player\|target\)\.[a-z_]\+\>/ contained
syntax match eqscriptFlowNumber /\<\d\+\>/ contained
syntax match eqscriptOperator /[-+*/%!=<>&|():]/ contained
syntax region eqscriptString start=/"/ skip=/\\./ end=/"/ oneline contained
syntax region eqscriptFlow matchgroup=eqscriptFlowKeyword start=/^\s*\%(let\|set\|if\|else\|repeat\|while\|end\)\ze\%(\s\|#\|$\)/ end=/$/ keepend contains=eqscriptFlowKeyword,eqscriptType,eqscriptValue,eqscriptField,eqscriptFlowNumber,eqscriptOperator,eqscriptString,eqscriptComment
syntax keyword eqscriptTodo TODO FIXME NOTE XXX contained

highlight default link eqscriptCommand Statement
highlight default link eqscriptNumber Number
highlight default link eqscriptKey Special
highlight default link eqscriptComment Comment
highlight default link eqscriptTodo Todo
highlight default link eqscriptFlowKeyword Statement
highlight default link eqscriptType Type
highlight default link eqscriptValue Constant
highlight default link eqscriptField Identifier
highlight default link eqscriptFlowNumber Number
highlight default link eqscriptOperator Operator
highlight default link eqscriptString String

let b:current_syntax = 'eqscript'
