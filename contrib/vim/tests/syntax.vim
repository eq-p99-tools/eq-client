" Run from the repository root: vim -Nu NONE -i NONE -n -es -S contrib/vim/tests/syntax.vim
set runtimepath^=contrib/vim
filetype plugin on
syntax enable
edit test.eqscript
call setline(1, ['press CTRL+I # note', 'wait 250', 'hold alt+1 50', '# TODO check', 'report wait', 'report "# still a comment'])
call assert_equal('eqscript', &filetype)
call assert_equal('eqscriptCommand', synIDattr(synID(1, 1, 1), 'name'))
call assert_equal('eqscriptKey', synIDattr(synID(1, 7, 1), 'name'))
call assert_equal('eqscriptComment', synIDattr(synID(1, 14, 1), 'name'))
call assert_equal('eqscriptNumber', synIDattr(synID(2, 6, 1), 'name'))
call assert_equal('eqscriptKey', synIDattr(synID(3, 6, 1), 'name'))
call assert_equal('eqscriptTodo', synIDattr(synID(4, 3, 1), 'name'))
call assert_notequal('eqscriptCommand', synIDattr(synID(5, 8, 1), 'name'))
call assert_equal('eqscriptComment', synIDattr(synID(6, 9, 1), 'name'))
call setline(7, ['let n: int = 1+2', 'if zone.name=="cat\"#1" # note', 'while n < 5 limit 3', 'end'])
call assert_equal('eqscriptFlowKeyword', synIDattr(synID(7, 1, 1), 'name'))
call assert_equal('eqscriptType', synIDattr(synID(7, 8, 1), 'name'))
call assert_equal('eqscriptFlowNumber', synIDattr(synID(7, 14, 1), 'name'))
call assert_equal('eqscriptOperator', synIDattr(synID(7, 15, 1), 'name'))
call assert_equal('eqscriptField', synIDattr(synID(8, 4, 1), 'name'))
call assert_equal('eqscriptString', synIDattr(synID(8, 20, 1), 'name'))
call assert_equal('eqscriptComment', synIDattr(synID(8, 25, 1), 'name'))
call assert_equal('eqscriptFlowKeyword', synIDattr(synID(9, 13, 1), 'name'))
call assert_equal('# %s', &commentstring)
edit! generic.txt
call assert_notequal('eqscript', &filetype)
if !empty(v:errors)
  for error in v:errors
    echomsg error
  endfor
  cquit
endif
qa!
