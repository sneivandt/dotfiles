execute pathogen#infect()
filetype indent plugin on
let mapleader=','
au BufWritePre * :%s/\s\+$//e
au VimResized * :wincmd =

""""""""""""""""""""""""""""""""""""""""
" Basic options

set autoindent
set backspace=2
set cindent
set cmdheight=1
set copyindent
set cursorline
set encoding=utf-8
set expandtab
set fileformats=unix,dos,mac
set formatoptions+=r
set hlsearch
set ignorecase
set incsearch
set laststatus=2
set lazyredraw
set nobackup
set nocompatible
set noerrorbells
set nonumber
set noswapfile
set pastetoggle=<f2>
set preserveindent
set ruler
set scrolloff=5
set shiftwidth=2
set showmatch
set showmode
set smartcase
set smartindent
set softtabstop=2
set splitbelow
set splitright
set tabstop=2
set textwidth=80
set ttyfast
set viminfo=
set wildmenu
set wrap
set wrapscan

""""""""""""""""""""""""""""""""""""""""
" Color scheme

syntax on
set t_Co=256
try
  colorscheme jellybeans
catch
  colorscheme default
endtry

""""""""""""""""""""""""""""""""""""""""
" GVIM

set guitablabel=%N\ %t\ %M
set guioptions-=m
set guioptions-=T
set guioptions-=r
set guioptions-=L

""""""""""""""""""""""""""""""""""""""""
" Wildignore

set wildignore+=*.so,*.swp,*.zip,*.min.*,*.o*
set wildignore+=*/tmp/*,*/node_modules/*,*/bower_components/*,*/.git/*,*/.hg/*,*/.svn/*

""""""""""""""""""""""""""""""""""""""""
" Languages

au Filetype c    setlocal shiftwidth=4 tabstop=4 softtabstop=4
au Filetype cpp  setlocal shiftwidth=4 tabstop=4 softtabstop=4
au Filetype java setlocal shiftwidth=4 tabstop=4 softtabstop=4


""""""""""""""""""""""""""""""""""""""""
" Airline

let g:airline_left_sep=''
let g:airline_left_alt_sep=''
let g:airline_right_sep=''
let g:airline_right_alt_sep=''

let g:airline#extensions#tabline#enabled=1
let g:airline#extensions#tabline#show_buffers=0
let g:airline#extensions#tabline#tab_min_count=2
let g:airline#extensions#tabline#tab_nr_type=1
let g:airline#extensions#tabline#left_sep=''
let g:airline#extensions#tabline#left_alt_sep=''
let g:airline#extensions#tabline#right_sep=''
let g:airline#extensions#tabline#right_alt_sep=''
let g:airline#extensions#tabline#fnamemod=':t'

let g:tmuxline_separators = { 'left': '', 'left_alt': '', 'right': '', 'right_alt': '', 'space': ' ' }

""""""""""""""""""""""""""""""""""""""""
" Ctrl-P

let g:ctrlp_map='<c-p>'
let g:ctrlp_cmd='CtrlP'
let g:ctrlp_working_path_mode='ra'

""""""""""""""""""""""""""""""""""""""""
" Nerdtree

let NERDTreeMinimalUI=1
let NERDTreeRespectWildIgnore=1
map <silent><c-e> :NERDTreeToggle<cr>:wincmd =<cr>

""""""""""""""""""""""""""""""""""""""""
" Tabular

map <silent><leader>a= :Tabularize /^[^=]*\zs=<cr>
map <silent><leader>a: :Tabularize /^[^:]*:\s*\zs\s/l0<cr>

""""""""""""""""""""""""""""""""""""""""
" Mappings

map <tab> %
map j gj
map k gk
nnoremap / /\v
nnoremap ? ?\v
nnoremap n nzzzv
nnoremap N Nzzzv
nnoremap * *<C-o>

map <f3> :setlocal spell! spelllang=en_us<CR>

map <c-r>n :tabnew<cr>
map <c-r><c-r> :tabnext<cr>
map <c-r>1 1gt
map <c-r>2 2gt
map <c-r>3 3gt
map <c-r>4 4gt
map <c-r>5 5gt
map <c-r>6 6gt
map <c-r>7 7gt
map <c-r>8 8gt
map <c-r>9 9gt

cmap W w !sudo tee % >/dev/null<cr>
nmap <leader>w :w!<cr>
nmap <leader>q :q!<cr>
nmap <leader>g :vimgrep //j ** <bar> cw<left><left><left><left><left><left><left><left><left><left>
nmap <leader>r :%s/\<<c-r><c-w>\>/
nmap <leader><space> :noh<cr>
