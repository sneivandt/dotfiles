execute 'source ' . fnameescape($DOTFILES_VIM_CONFIG)
setglobal list foldlevel=4
setlocal list foldlevel=4

file audit-go
setfiletype go
let go_buffer = bufnr()
call assert_equal(0, &l:list, 'Go hides whitespace')
call assert_equal(1, &g:list, 'Go does not change the global default')

enew
setfiletype python
call assert_equal(1, &l:list, 'A new non-Go buffer restores whitespace')
execute 'buffer ' . go_buffer
call assert_equal(0, &l:list, 'Re-entering Go reapplies its option')

split
enew
setfiletype python
call assert_equal(1, &l:list, 'A split does not inherit Go whitespace policy')
wincmd p
call assert_equal(0, &l:list, 'The original Go window keeps its policy')
close

file audit-snippets
set filetype=snippets
let snippets_buffer = bufnr()
call assert_equal(0, &l:foldlevel, 'Snippets start folded')
call assert_equal(4, &g:foldlevel, 'Snippets do not change the global default')
enew
setfiletype python
call assert_equal(4, &l:foldlevel, 'A new non-snippet buffer restores fold level')
execute 'buffer ' . snippets_buffer
call assert_equal(0, &l:foldlevel, 'Re-entering snippets reapplies folding')
set filetype=go
call assert_equal(4, &l:foldlevel, 'Changing filetype restores snippet folding')
call assert_equal(0, &l:list, 'Changing filetype applies Go whitespace policy')
set filetype=python
call assert_equal(1, &l:list, 'Changing filetype restores Go whitespace policy')

setlocal nolist foldlevel=7
doautocmd BufWinEnter
call assert_equal(0, &l:list, 'Unrelated local whitespace preferences survive')
call assert_equal(7, &l:foldlevel, 'Unrelated local folding preferences survive')
call assert_equal(1, &g:list, 'Global whitespace remains unchanged')
call assert_equal(4, &g:foldlevel, 'Global folding remains unchanged')

" Fold labels count all lines and fit display columns rather than UTF-8 bytes.
set encoding=utf-8
enew
setlocal foldmethod=marker foldlevel=0
for header in [repeat('界', 100), repeat("e\u0301", 100), "\t" . repeat('wide 界 ', 40)]
  call setline(1, [header . ' {{{', 'body', '}}}'])
  normal! zM
  let label = foldtextresult(1)
  call assert_match('3 $', label, 'The fold includes both boundary lines')
  call assert_equal(winwidth(0) - getwininfo(win_getid())[0].textoff,
        \ strdisplaywidth(label), 'Unicode fold labels fill the text area')
endfor
vsplit
vertical resize 12
let label = foldtextresult(1)
call assert_equal(winwidth(0) - getwininfo(win_getid())[0].textoff,
      \ strdisplaywidth(label), 'Fold labels fit narrow splits')
close

if !empty(v:errors)
  for message in v:errors
    echoerr message
  endfor
  cquit
endif
qa!
