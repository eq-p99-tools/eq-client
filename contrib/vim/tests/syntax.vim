" Run from the repository root: vim -Nu NONE -i NONE -n -es -S contrib/vim/tests/syntax.vim
set runtimepath^=contrib/vim
filetype plugin indent on
syntax enable
edit test.eqscript
call setline(1, ['press("CTRL+I") # note', 'wait(250)', 'hold(''alt+1'', 50)', '# TODO check', 'report("wait")', 'report("# still text")'])
call assert_equal('eqscript', &filetype)
call assert_equal('eqscriptCommand', synIDattr(synID(1, 1, 1), 'name'))
call assert_equal('eqscriptKey', synIDattr(synID(1, 8, 1), 'name'))
call assert_equal('eqscriptComment', synIDattr(synID(1, 17, 1), 'name'))
call assert_equal('eqscriptNumber', synIDattr(synID(2, 6, 1), 'name'))
call assert_equal('eqscriptKey', synIDattr(synID(3, 7, 1), 'name'))
call assert_equal('eqscriptNumber', synIDattr(synID(3, 15, 1), 'name'))
call assert_equal('eqscriptTodo', synIDattr(synID(4, 3, 1), 'name'))
call assert_equal('eqscriptString', synIDattr(synID(5, 9, 1), 'name'))
call assert_equal('eqscriptString', synIDattr(synID(6, 10, 1), 'name'))
call setline(7, ['n: int = 1+2', 'if zone.name == "cat\"#1" and not False: # note', 'while n < 5, limit=3:', '    break', 'for i in range(3):', 'def f(n: int):'])
call assert_equal('eqscriptType', synIDattr(synID(7, 4, 1), 'name'))
call assert_equal('eqscriptNumber', synIDattr(synID(7, 10, 1), 'name'))
call assert_equal('eqscriptOperator', synIDattr(synID(7, 11, 1), 'name'))
call assert_equal('eqscriptKeyword', synIDattr(synID(8, 1, 1), 'name'))
call assert_equal('eqscriptField', synIDattr(synID(8, 4, 1), 'name'))
call assert_equal('eqscriptString', synIDattr(synID(8, 22, 1), 'name'))
call assert_equal('eqscriptKeyword', synIDattr(synID(8, 27, 1), 'name'))
call assert_equal('eqscriptConstant', synIDattr(synID(8, 35, 1), 'name'))
call assert_equal('eqscriptComment', synIDattr(synID(8, 43, 1), 'name'))
call assert_equal('eqscriptKeyword', synIDattr(synID(9, 14, 1), 'name'))
call assert_equal('eqscriptKeyword', synIDattr(synID(10, 5, 1), 'name'))
call assert_equal('eqscriptBuiltin', synIDattr(synID(11, 10, 1), 'name'))
call assert_equal('eqscriptKeyword', synIDattr(synID(12, 1, 1), 'name'))
call assert_equal('eqscriptDefinition', synIDattr(synID(12, 5, 1), 'name'))
call assert_equal('eqscriptType', synIDattr(synID(12, 10, 1), 'name'))
call setline(13, ['create_refused("x")', 'wait_relocation(10)', 'boat("acquire")', 'press I'])
call assert_equal('eqscriptCommand', synIDattr(synID(13, 8, 1), 'name'))
call assert_equal('eqscriptCommand', synIDattr(synID(14, 6, 1), 'name'))
call assert_equal('eqscriptCommand', synIDattr(synID(15, 1, 1), 'name'))
" A word without its call is not coloured as a command.
call assert_notequal('eqscriptCommand', synIDattr(synID(16, 1, 1), 'name'))
call assert_equal('# %s', &commentstring)
call assert_equal(4, &shiftwidth)
" Indentation follows the blocks.
%delete _
call setline(1, ['if client.ready: # "#:"', 'press("I")', 'elif False:', 'pass', 'else:', 'for _ in range(2):', 'wait(1)', 'break', 'report("done")'])
normal! gg=G
call assert_equal([0, 4, 0, 4, 0, 4, 8, 8, 4], map(range(1, 9), 'indent(v:val)'))
edit! generic.txt
call assert_notequal('eqscript', &filetype)
if !empty(v:errors)
  for error in v:errors
    echomsg error
  endfor
  cquit
endif
qa!
