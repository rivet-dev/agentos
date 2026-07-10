;===========================================================================
; Copyright (c) 1990-2002 Info-ZIP.  All rights reserved.
;
; See the accompanying file LICENSE, version 2000-Apr-09 or later
; (the contents of which are also included in unzip.h) for terms of use.
; If, for some reason, all these files are missing, the Info-ZIP license
; also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
;===========================================================================
; flate.a created by Paul Kienitz, 20 June 94.  Last modified 23 Mar 2002.
;
; 68000 assembly language version of inflate_codes(), for Amiga.  Prototype:
;
;   int inflate_codes(__GPRO__ struct huft *tl, struct huft *td,
;                     unsigned bl, unsigned bd);
;
; Where __GPRO__ expands to "Uz_Globs *G," if REENTRANT is defined,
; otherwise to nothing.  In the latter case G is a global variable.
;
; Define the symbol FUNZIP if this is for fUnZip.  It overrides REENTRANT.
;
; Define AZTEC to use the Aztec C macro version of getc() instead of the
; library getc() with FUNZIP.  AZTEC is ignored if FUNZIP is not defined.
;
; Define NO_CHECK_EOF to not use the fancy paranoid version of NEEDBITS --
; this is equivalent to removing the #define CHECK_EOF from inflate.c.
;
; Define INT16 if ints are short, otherwise it assumes ints are long.
;
; Define USE_DEFLATE64 if we're supporting Deflate64 decompression.
;
; Do NOT define WSIZE; it is always 32K or 64K depending on USE_DEFLATE64.
;
; 1999/09/23: for Human68k: Modified by Shimazaki Ryo.

X:              EQU     $7ffe

                IFDEF   INT16
MOVINT           MACRO  _1,_2
        move.w          _1,_2
                 ENDM
INTSIZE equ     2
                ELSE    ; !INT16
MOVINT           MACRO  _1,_2
        move.l          _1,_2
                 ENDM
INTSIZE equ     4
                ENDC

                IFDEF   REENTRANT
                 IFNDEF FUNZIP
REENT_G equ     1
                 ENDC
                ENDC

; The following include file is generated from globals.h, and gives us equates
; that give the offsets in Uz_Globs of the fields we use, which are:
;       ulg bb
;       unsigned int bk, wp
;       (either array of unsigned char, or pointer to unsigned char) redirslide
; For fUnZip:
;       FILE *in
; For regular UnZip but not fUnZip:
;       int incnt, mem_mode
;       long csize
;       uch *inptr
; It also defines a value SIZEOF_slide, which tells us whether the appropriate
; slide field in G (either area.Slide or redirect_pointer) is a pointer or an
; array instance.  It is 4 in the former case and a large value in the latter.
; Lastly, this include will define CRYPT as 1 if appropriate.

                IFDEF   FUNZIP
        INCLUDE  human68k/G_offs_.mac
                ELSE
                 IFDEF  SFX
        INCLUDE  human68k/G_offsf.mac"
                 ELSE
        INCLUDE  human68k/G_offs.mac
                 ENDC
                ENDC

; struct huft is defined as follows:
;
;   struct huft {
;     uch e;                /* number of extra bits or operation */
;     uch b;                /* number of bits in this code or subcode */
;     union {
;       ush n;              /* literal, length base, or distance base */
;       struct huft *t;     /* pointer to next level of table */
;     } v;
;   };                      /* sizeof(struct huft) == 6 */
;
; The G_offs include defines offsets h_e, h_b, h_v_n, and h_v_t in this
; struct, plus SIZEOF_huft.

; G.bb is the global buffer that holds bits from the huffman code stream, which
; we cache in the register variable b.  G.bk is the number of valid bits in it,
; which we cache in k.  The macros NEEDBITS(n) and DUMPBITS(n) have side effects
; on b and k.

                IFDEF   REENT_G
G_SIZE  equ     4
G_PUSH           MACRO          ; this macro passes "__G__" to functions
        move.l          G,-(sp)
                 ENDM
                ELSE
        xref    _G              ; Uz_Globs
G_SIZE  equ     0
G_PUSH           MACRO
        ds.b            0       ; does nothing; the assembler dislikes MACRO ENDM
                 ENDM
                ENDC    ; REENT_G

;;      xref    _mask_bits      ; const unsigned mask_bits[17];
                IFDEF   FUNZIP
                 IF     CRYPT
        xref    _encrypted      ; int -- boolean flag
        xref    _update_keys    ; int update_keys(__GPRO__ int)
        xref    _decrypt_byte   ; int decrypt_byte(__GPRO)
                 ENDC   ; CRYPT
                ELSE    ; !FUNZIP
        xref    _memflush       ; int memflush(__GPRO__ uch *, ulg)
        xref    _readbyte       ; int readbyte(__GPRO)
                ENDC    ; FUNZIP

        xref    _flush          ; if FUNZIP:  int flush(__GPRO__ ulg)
                                ; else:  int flush(__GPRO__ uch *, ulg, int)

; Here are our register variables.

b       reg     d2              ; unsigned long
k       reg     d3              ; unsigned short <= 32
e       reg     d4              ; unsigned int, mostly used as unsigned char
w       reg     d5              ; unsigned long (was short before deflate64)
n       reg     d6              ; unsigned long (was short before deflate64)
d       reg     d7              ; unsigned int, used as unsigned short

t       reg     a2              ; struct huft *
lmask   reg     a3              ; ulg *
G       reg     a6              ; Uz_Globs *

; Couple other items we need:

savregs reg     d2-d7/a2/a3/a6
                IFDEF   USE_DEFLATE64
WSIZE   equ     $10000          ; 64K... be careful not to treat as short!
                ELSE
WSIZE   equ     $08000          ; 32K... be careful not to treat as negative!
                ENDC
EOF     equ     -1
INVALID equ     99

; inflate_codes() returns one of the following status codes:
;          0  OK
;          1  internal inflate error or EOF on input stream
;         the following return codes are passed through from FLUSH() errors
;          50 (PK_DISK)   "overflow of output space"
;          80 (IZ_CTRLC)  "canceled by user's request"

RET_OK  equ     0
RET_ERR equ     1

                IFDEF   FUNZIP
; This does getc(in).  LIBC version is based on #define getc(fp) in stdio.h

GETC              MACRO
        xref    _fgetc          ; int fgetc(FILE *)
        move.l          in-X(G),-(sp)
        jsr             _fgetc
        addq.l          #4,sp
                  ENDM
                ENDC    ; FUNZIP

; Input depends on the NEXTBYTE macro.  This exists in three different forms.
; The first two are for fUnZip, with and without decryption.  The last is for
; regular UnZip with or without decryption.  The resulting byte is returned
; in d0 as a longword, and d1, a0, and a1 are clobbered.

; FLUSH also has different forms for UnZip and fUnZip.  Arg must be a longword.
; The same scratch registers are trashed.

                IFDEF   FUNZIP

NEXTBYTE         MACRO
        move.l   d2,-(sp)
        GETC
                  IF    CRYPT
        tst.w           _encrypted+INTSIZE-2    ; test low word if long
        beq.s           @nbe
        MOVINT          d0,-(sp)                ; save thru next call
        G_PUSH
        jsr             _decrypt_byte
        eor.w           d0,G_SIZE+INTSIZE-2(sp) ; becomes arg to update_keys
        jsr             _update_keys
        addq            #INTSIZE+G_SIZE,sp
@nbe:
                  ENDC  ; !CRYPT
                  IFEQ INTSIZE-2
        ext.l           d0              ; assert -1 <= d0 <= 255
                  ENDC
        move.l   (sp)+,d2
                 ENDM

FLUSH            MACRO  _1
        move.l          d2,-(sp)
        move.l          _1,-(sp)
        G_PUSH
        jsr             _flush
        addq            #4+G_SIZE,sp
        move.l          (sp)+,d2
                 ENDM

                ELSE    ; !FUNZIP

NEXTBYTE         MACRO
        subq.w          #1,incnt+INTSIZE-2-X(G)   ; treat as short
        bge.s           @nbs
                IFNE INTSIZE-2
        subq.w          #1,incnt-X(G)
        bge.s           @nbs
                ENDIF
        move.l          d2,-(sp)
        G_PUSH
        jsr             _readbyte
                  IFNE G_SIZE
        addq            #G_SIZE,sp
                  ENDC
        move.l          (sp)+,d2
                  IFEQ 2-INTSIZE
        ext.l           d0            ; assert -1 <= d0 <= 255
                  ENDC
        bra.s           @nbe
@nbs:   moveq           #0,d0
        move.l          inptr-X(G),a0
        move.b          (a0)+,d0
        move.l          a0,inptr-X(G)
@nbe:
                 ENDM

FLUSH            MACRO  _1
        move.l          d2,-(sp)
        clr.l           -(sp)                   ; unshrink flag: always false
        move.l          _1,-(sp)                ; length
                  IF    SIZEOF_slide>4
        pea             redirslide-X(G)           ; buffer to flush
                  ELSE
        move.l          redirslide-X(G),-(sp)
                  ENDC
        G_PUSH
        tst.w           mem_mode+INTSIZE-2-X(G)   ; test lower word if long
        beq.s           @fm
        jsr             _memflush               ; ignores the unshrink flag
        bra.s           @fe
@fm:    jsr             _flush
@fe:    lea             8+INTSIZE+G_SIZE(sp),sp
        move.l          (sp)+,d2
                 ENDM

                ENDC    ; ?FUNZIP

; Here are the two bit-grabbing macros, defined in their NO_CHECK_EOF form:
;
;   #define NEEDBITS(n) {while(k<(n)){b|=((ulg)NEXTBYTE)<<k;k+=8;}}
;   #define DUMPBITS(n) {b>>=(n);k-=(n);}
;
; Without NO_CHECK_EOF, NEEDBITS reads like this:
;
;   {while((int)k<(int)(n)){int c=NEXTBYTE;
;                           if(c==EOF){if((int)k>=0)break;return 1};
;                           b|=((ulg)c)<<k;k+=8;}}
;
; NEEDBITS clobbers d0, d1, a0, and a1, none of which can be used as the arg to
; the macro specifying the number of bits.  The arg can be a shortword memory
; address, or d2-d7.  The result is copied into d1 as a word ready for masking.
; DUMPBITS has no side effects; the arg must be a d-register (or immediate in
; the range 1-8?) and only the lower byte is significant.

NEEDBITS        MACRO   _1
@nb:    cmp.w           _1,k            ; assert 0 < k <= 32 ... arg may be 0
        bge.s           @ne             ; signed compare!
@loop:
        NEXTBYTE                        ; returns in d0.l
                 IFNDEF NO_CHECK_EOF
        cmp.w           #EOF,d0
        bne.s           @nok
        tst.w           k
        bge.s           @ne
        bra             error_return
                 ENDC   ; !NO_CHECK_EOF
@nok:   lsl.l           k,d0
        or.l            d0,b
        addq.w          #8,k
        cmp.w           _1,k            ;bra.s @nb
        bcs             @loop           ;
@ne:    move.l          b,d1            ; return a copy of b in d1
                ENDM

DUMPBITS        MACRO   _1
        lsr.l           _1,b            ; upper bits of _1 are ignored??
        sub.b           _1,k
                ENDM


; This is a longword version of the mask_bits constant array:
longmasks:      dc.l    $00000000,$00000001,$00000003,$00000007,$0000000F
                dc.l    $0000001F,$0000003F,$0000007F,$000000FF,$000001FF
                dc.l    $000003FF,$000007FF,$00000FFF,$00001FFF,$00003FFF
                dc.l    $00007FFF,$0000FFFF,0,0,0,0,0,0,0,0,0,0,0,0,0,0


; ******************************************************************************
; Here we go, finally:

        xdef    _inflate_codes

_inflate_codes:
        link            a5,#-8
        movem.l         savregs,-(sp)
; 8(a5) = tl, 12(a5) = td, 16(a5) = bl, 18|20(a5) = bd... add 4 for REENT_G
; -4(a5) = ml, -8(a5) = md, both unsigned long.
; Here we cache some globals and args:
                IFDEF   REENT_G
        move.l          8(a5),G
                ELSE
        lea             _G,G            ; G is now a global instance
                IFDEF   X
        lea             (X,G),G
                ENDIF
                ENDC
        lea             longmasks,lmask
        move.l          bb-X(G),b
        MOVINT          bk-X(G),k
                IFDEF   INT16
        moveq           #0,w            ; keep this usable as longword
                ENDC
        MOVINT          wp-X(G),w
        moveq           #0,e            ; keep this usable as longword too
        MOVINT          16+G_SIZE(a5),d0
        asl.w           #2,d0
        move.l          (lmask,d0.w),-4(a5)     ; ml = mask_bits[bl]
        MOVINT          16+INTSIZE+G_SIZE(a5),d0
        asl.w           #2,d0
        move.l          (lmask,d0.w),-8(a5)     ; md = mask_bits[bd]

main_loop:
        NEEDBITS        14+INTSIZE+G_SIZE(a5)   ; (unsigned) bl
        and.l           -4(a5),d1               ; ml
                IFNE SIZEOF_huft-8
        mulu            #SIZEOF_huft,d1
                ELSE
        asl.l           #3,d1
                ENDC
        move.l          8+G_SIZE(a5),t          ; tl
        add.l           d1,t
newtop:  move.b         h_b(t),d0
         DUMPBITS       d0
         move.b         h_e(t),e
         cmp.b          #32,e                   ; is it a literal?
         bne            nonlit                  ; no
          move.w        h_v_n(t),d0             ; yes
                IFGT SIZEOF_slide-4
          lea           redirslide-X(G),a0
                ELSE
          move.l        redirslide-X(G),a0
                ENDC
          move.b        d0,(a0,w.l)             ; stick in the decoded byte
          addq.l        #1,w
          cmp.l         #WSIZE,w
          blo           main_loop
           FLUSH        w
           ext.l        d0                      ; does a test as it casts long
           bne          return
           moveq        #0,w
           bra          main_loop               ; break (newtop loop)

nonlit:  cmp.b          #31,e                   ; is it a length?
         beq            finish                  ; no, it's the end marker
         bhi            nonleng                 ; no, it's something else
          NEEDBITS      e                       ; yes: a duplicate string
          move.w        e,d0
          asl.w         #2,d0
          and.l         (lmask,d0.w),d1
          moveq         #0,n                    ; cast h_v_n(t) to long
          move.w        h_v_n(t),n
          add.l         d1,n                    ; length of block to copy
          DUMPBITS      e
          NEEDBITS      14+(2*INTSIZE)+G_SIZE(a5)   ; bd, lower word if long
          and.l         -8(a5),d1                   ; md
                IFNE SIZEOF_huft-8
          mulu          #SIZEOF_huft,d1
                ELSE
          asl.l         #3,d1
                ENDC
          move.l        12+G_SIZE(a5),t                 ; td
          add.l         d1,t
distop:    move.b       h_b(t),d0
           DUMPBITS     d0
           move.b       h_e(t),e
           cmp.b        #32,e                   ; is it a literal?
           blo.s        disbrk                  ; then stop doing this
            cmp.b       #INVALID,e              ; is it bogus?
            bne.s       disgo
             bra        error_return            ; then fail
disgo:      and.w       #$001F,e
            NEEDBITS    e
            move.w      e,d0
            asl.w       #2,d0
            and.l       (lmask,d0.w),d1
                IFNE SIZEOF_huft-8
            mulu        #SIZEOF_huft,d1
                ELSE
            asl.l       #3,d1
                ENDC
            move.l      h_v_t(t),t
            add.l       d1,t
            bra         distop
disbrk:   NEEDBITS      e
          move.l        e,d0
          asl.w         #2,d0
          and.l         (lmask,d0.w),d1
          move.l        w,d
          move.w        h_v_n(t),d0     ; assert top word of d0 is zero
          sub.l         d0,d
          sub.l         d1,d            ; distance back to copy the block
          DUMPBITS      e

docopy:    move.l       #WSIZE,e        ; copy the duplicated string
           and.l        #WSIZE-1,d      ; ...but first check if the length
           cmp.l        d,w             ; will overflow the window...
           blo.s        ddgw
            sub.l       w,e
           bra.s        dadw
ddgw:       sub.l       d,e
dadw:      cmp.l        #$08000,e       ; also, only copy <= 32K, so we can
           bls.s        dnox            ; use a dbra loop to do it
            move.l      #$08000,e
dnox:      cmp.l        n,e
           bls.s        delen
            move.l      n,e
delen:     sub.l        e,n             ; size of sub-block to copy in this pass
                IF      SIZEOF_slide>4
           lea          redirslide-X(G),a0
                ELSE
           move.l       redirslide-X(G),a0
                ENDC
           move.l       a0,a1
           add.l        w,a0            ; w and d are valid longwords
           add.l        d,a1
; Now at this point we could do tests to see if we should use an optimized
; large block copying method such as movem's, but since (a) such methods require
; the source and destination to be compatibly aligned -- and odd bytes at each
; end have to be handled separately, (b) it's only worth checking for if the
; block is pretty large, and (c) most strings are only a few bytes long, we're
; just not going to bother.  Therefore we check above to make sure we move at
; most 32K in one sub-block, so a dbra loop can handle it.
dshort:    move.l       e,d0
           subq         #1,d0           ; assert >= 0
dspin:      move.b      (a1)+,(a0)+
            dbra        d0,dspin
           add.l        e,w
           add.l        e,d
           cmp.l        #WSIZE,w
           blo.s        dnfl
            FLUSH       w
            ext.l       d0              ; does a test as it casts to long
            bne         return
            moveq       #0,w
dnfl:      tst.l        n               ; need to do more sub-blocks?
           bne          docopy          ; yes
          moveq         #0,e            ; restore zeroness in upper bytes of e
          bra           main_loop       ; break (newtop loop)

nonleng: cmp.w          #INVALID,e      ; bottom of newtop loop -- misc. code
         bne.s          tailgo          ; invalid code?
          bra           error_return    ; then fail
tailgo:  and.w          #$001F,e
         NEEDBITS       e
         move.w         e,d0
         asl.w          #2,d0
         and.l          (lmask,d0.w),d1
                IFNE SIZEOF_huft-8
         mulu           #SIZEOF_huft,d1
                ELSE
         asl.l          #3,d1
                ENDC
         move.l         h_v_t(t),t
         add.l          d1,t
         bra            newtop

finish: MOVINT          w,wp-X(G)       ; done: restore cached globals
        MOVINT          k,bk-X(G)
        move.l          b,bb-X(G)
        moveq           #RET_OK,d0      ; return "no error"
return: movem.l         (sp)+,savregs
        unlk            a5
        rts

error_return:
        moveq           #RET_ERR,d0     ; return "error occured"
        bra             return
