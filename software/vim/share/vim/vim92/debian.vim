" Normally we use vim-extensions. If you want true vi-compatibility
" remove change the following statements
set nocompatible	" Use Vim defaults instead of 100% vi compatibility
set backspace=indent,eol,start	" more powerful backspacing

" Now we set some defaults for the editor
set history=50		" keep 50 lines of command line history
set ruler		" show the cursor position all the time

" modelines have historically been a source of security/resource
" vulnerabilities -- disable by default, even when 'nocompatible' is set
set nomodeline

" Suffixes that get lower priority when doing tab completion for filenames.
" These are files we are not likely to want to edit or read.
set suffixes=.bak,~,.swp,.o,.info,.aux,.log,.dvi,.bbl,.blg,.brf,.cb,.ind,.idx,.ilg,.inx,.out,.toc

" We know xterm-debian is a color terminal
if &term =~ "xterm-debian" || &term =~ "xterm-xfree86"
  set t_Co=16
  set t_Sf=[3%dm
  set t_Sb=[4%dm
endif

" Some Debian-specific things
if has('gui')
  " Must define this within the :if so it does not cause problems with
  " vim-tiny (which does not have +eval)
  function! <SID>MapExists(name, modes)
    for mode in split(a:modes, '\zs')
      if !empty(maparg(a:name, mode))
        return 1
      endif
    endfor
    return 0
  endfunction

  " Make shift-insert work like in Xterm
  autocmd GUIEnter * if !<SID>MapExists("<S-Insert>", "nvso") | execute "map <S-Insert> <MiddleMouse>" | endif
  autocmd GUIEnter * if !<SID>MapExists("<S-Insert>", "ic") | execute "map! <S-Insert> <MiddleMouse>" | endif
endif

" Set paper size from /etc/papersize if available (Debian-specific)
if filereadable("/etc/papersize")
  let s:papersize = matchstr(readfile('/etc/papersize', '', 1), '\p*')
  if strlen(s:papersize)
    exe "set printoptions+=paper:" . s:papersize
  endif
endif

