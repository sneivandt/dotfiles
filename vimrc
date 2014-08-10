execute pathogen#infect()
filetype indent plugin on
set nocompatible
set fileformats=unix,dos,mac
set viminfo=
set noswapfile
set nobackup
let mapleader=','
au BufWritePre * :%s/\s\+$//e
au VimResized * :wincmd =


""""""""""""""""""""""""""""""""""""""""
" Basic options
""""""""""""""""""""""""""""""""""""""""
set autoindent
set cindent
set cmdheight=1
set copyindent
set cursorline
set encoding=utf-8
set expandtab
set formatoptions+=r
set hlsearch
set ignorecase
set incsearch
set laststatus=2
set noerrorbells
set nonumber
set pastetoggle=<F2>
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
set ttyfast
set wildmenu
set wrap
set wrapscan


""""""""""""""""""""""""""""""""""""""""
" Color scheme
""""""""""""""""""""""""""""""""""""""""
syntax on
set t_Co=256
try
   colorscheme jellybeans
catch
   colorscheme default
endtry


""""""""""""""""""""""""""""""""""""""""
" GVIM
""""""""""""""""""""""""""""""""""""""""
set guitablabel=%N\ %t\ %M
set guioptions-=m
set guioptions-=T
set guioptions-=r
set guioptions-=L


""""""""""""""""""""""""""""""""""""""""
" Wildignore
""""""""""""""""""""""""""""""""""""""""
set wildignore+=*/tmp/*,*.so,*.swp,*.zip,*.min.*
set wildignore+=*/node_modules/*,*/bower_components/*,*/.git/*,*/.hg/*,*/.svn/*


""""""""""""""""""""""""""""""""""""""""
" Airline
""""""""""""""""""""""""""""""""""""""""
let g:airline_left_sep=''
let g:airline_left_alt_sep=''
let g:airline_right_sep=''
let g:airline_right_alt_sep=''
let g:tmuxline_separators = {
  \ 'left'      : '',
  \ 'left_alt'  : '',
  \ 'right'     : '',
  \ 'right_alt' : '',
  \ 'space'     : ' '}


""""""""""""""""""""""""""""""""""""""""
" Tabline
""""""""""""""""""""""""""""""""""""""""
let g:airline#extensions#tabline#enabled=1
let g:airline#extensions#tabline#tab_nr_type=1
let g:airline#extensions#tabline#left_sep=''
let g:airline#extensions#tabline#left_alt_sep=''
let g:airline#extensions#tabline#right_sep=''
let g:airline#extensions#tabline#right_alt_sep=''
map <C-r>n :tabnew<CR>
map <C-r><C-r> :tabnext<CR>
map <C-r>1 1gt
map <C-r>2 2gt
map <C-r>3 3gt
map <C-r>4 4gt
map <C-r>5 5gt
map <C-r>6 6gt
map <C-r>7 7gt
map <C-r>8 8gt
map <C-r>9 9gt


""""""""""""""""""""""""""""""""""""""""
" Ctrl-P
""""""""""""""""""""""""""""""""""""""""
let g:ctrlp_map='<c-p>'
let g:ctrlp_cmd='CtrlP'
let g:ctrlp_working_path_mode='ra'


""""""""""""""""""""""""""""""""""""""""
" Nerdtree
""""""""""""""""""""""""""""""""""""""""
map <silent> <C-E> :NERDTreeToggle<CR>:wincmd =<CR>


""""""""""""""""""""""""""""""""""""""""
" Tabular
""""""""""""""""""""""""""""""""""""""""
map <silent> <Leader>a= :Tabularize /^[^=]*\zs=<CR>


""""""""""""""""""""""""""""""""""""""""
" Mappings
""""""""""""""""""""""""""""""""""""""""
map <tab> %
map j gj
map k gk
cmap W w !sudo tee % >/dev/null<CR>
nmap <Leader>w :w!<cr>
nmap <Leader>q :q!<cr>
nmap <Leader>g :vimgrep //j **<Bar> cw<Left><Left><Left><Left><Left><Left><Left><Left><Left>
nmap <Leader>r :%s/\<<C-r><C-w>\>/
nmap <Leader><Space> :noh<CR>
nnoremap / /\v
nnoremap ? ?\v
nnoremap n nzzzv
nnoremap N Nzzzv
nnoremap * *<C-O>
