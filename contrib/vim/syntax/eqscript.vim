" Vim/Neovim syntax for the EQ client scenario language. License: MIT.
if exists('b:current_syntax')
  finish
endif

syntax case match
syntax match eqscriptCommand /^\s*\zs\%(login\|wait_servers\|server\|wait_select\|select\|delete\|create\|create_refused\|wait_online\|wait_zone\|wait_relocation\|wait_dead\|slash\|boat\|social\|gm\|chat\|drag\|press\|hold\|wheel\|look\|wait\|camera\|trace\|face\|move\|approach\|chase\|walk\|give\|interact\|popup\|inspect_player\|inspect_link\|compose_link\|compose_owned_link\|click\|right_click\|hover\|hold_click\|report\|sound\|screenshot\|quit\)\>\ze\s*(/
syntax keyword eqscriptKeyword if elif else for in while break continue pass and or not def return
" The name a def line gives its function.
syntax match eqscriptDefinition /\%(^def\s\+\)\@<=\h\w*/
syntax match eqscriptKeyword /\<limit\ze\s*=/
syntax keyword eqscriptType int bool text
syntax keyword eqscriptConstant True False
syntax keyword eqscriptBuiltin known stale range
syntax match eqscriptField /\<\%(client\|zone\|player\|target\)\.[a-z_]\+\>/
syntax match eqscriptNumber /\<\d\+\%(\.\d\+\)\=\%([eE][-+]\=\d\+\)\=\>/
syntax match eqscriptOperator /[-+*/%=<>!,:()]/
syntax region eqscriptString start=/"/ skip=/\\./ end=/"/ oneline
syntax region eqscriptString start=/'/ skip=/\\./ end=/'/ oneline
" The first argument of press and hold is a key or chord.
syntax match eqscriptKey /\%(^\s*\%(press\|hold\)\s*(\s*\)\@<=\(["']\)[^"']*\1/
syntax match eqscriptComment /#.*/ contains=eqscriptTodo,@Spell
syntax keyword eqscriptTodo TODO FIXME NOTE XXX contained

highlight default link eqscriptCommand Function
highlight default link eqscriptKeyword Statement
highlight default link eqscriptDefinition Function
highlight default link eqscriptType Type
highlight default link eqscriptConstant Constant
highlight default link eqscriptBuiltin Function
highlight default link eqscriptField Identifier
highlight default link eqscriptNumber Number
highlight default link eqscriptOperator Operator
highlight default link eqscriptString String
highlight default link eqscriptKey Special
highlight default link eqscriptComment Comment
highlight default link eqscriptTodo Todo

let b:current_syntax = 'eqscript'
