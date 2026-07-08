/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
#include <qdos.h>
#include <limits.h>
#include <string.h>

struct
{
    short flag1;
    short flag2;
    long offset;            // The offset from &ds to unzipsfx exe
    long sfxlen;            // size of unzipsfx program
    long sfxdsiz;           // data size of unzipsfx program
    long sfxnam;            // Name offset from start of sfxprog
    long ziplen;            // size of zip file
} ds = {0x4afb, 0x4afb, 0};


typedef struct {short len; char chrs[1];} __QP_t;

#define __QA(l) struct {short len; char chrs[(l)+1];}

#define T1 \
   "\nThis is a self-extracting zip archive. In order to process the\n" \
   "archive you will be asked to give the name of a temporary directory\n" \
   "which must have at least as much free space as this SFX file.\n\n" \
   "You will also be asked for the name of the directory to which the\n" \
   "files are extracted. This directory _MUST_ exist. If you do not give\n" \
   "an output directory, the current default is used.\n\n\n" \
   "Device/directory for temporary files: "
#define T2 "Device/directory to extract to      : "

#define T1LEN (sizeof(T1)-1)
#define T2LEN (sizeof(T2)-1)

static void xgetcwd (__QP_t *s)
{
    extern char *_sys_var;
    static __QP_t **q;

    if (q = (__QP_t ** q) (_sys_var + 0xAC + 4))
    {
        memcpy (s->chrs, (*q)->chrs, (*q)->len);
        s->len = (*q)->len;
        *(s->chrs+s->len) = 0;
    }
}

int checkdir(__QP_t *dir)
{
    qdirect_t s;
    int r,ch;

    if(dir->len > 1)
    {
        if(*(dir->chrs + dir->len-2) == '_')
        {
            *(dir->chrs + dir->len-1) = 0;
            dir->len--;
        }
        else
        {
            *(dir->chrs + dir->len-1) = '_';
        }
    }
    else
    {
        xgetcwd(dir);
    }

    r = ERR_NF;

    if((ch = io_open(dir->chrs, 4)) > 0)
    {
        if((r = fs_headr(ch, -1, &s, sizeof(s))) > 0)
        {
            r = (s.d_type == 0xff) ? 0 : ERR_NF;
        }
        io_close(ch);
    }
    return r;
}

int makesfx(__QP_t *tmp)
{
    char *p = (char *)&ds;
    char *q;
    char txt[PATH_MAX];
    int fd,r = 0;
    qdirect_t qd;

    memcpy(txt, tmp->chrs, tmp->len);
    memcpy(txt+tmp->len, "SFX_EXE", 8);

    q = p + ds.offset;
    if((fd = io_open(txt, NEW_OVER)) > 0)
    {
        memcpy(txt+tmp->len+4, "DAT", 4);
        memcpy(q+ds.sfxnam, txt, tmp->len+8);
        fs_save(fd, q, ds.sfxlen);
        qd.d_length = ds.sfxlen;
        qd.d_datalen = ds.sfxdsiz;
        qd.d_type = 1;
        fs_heads(fd, -1, &qd, sizeof(qd));
        io_close(fd);
        if((fd = io_open(txt, NEW_OVER)) > 0)
        {
            q += ds.sfxlen;
            fs_save(fd, q, ds.ziplen);
            io_close(fd);
        }
        else r = fd;
    }
    else r = fd;
    return r;
}

#define T3 "\n\nTo extract the files, run the command \"LRUN "
#define T4 "Press any key to exit "
#define T3LEN (sizeof(T3)-1)
#define T4LEN (sizeof(T4)-1)

int unpackit ( __QP_t *tmpdir, __QP_t *outdir, char *basfil, int con)
{
    int ch, r = 0;
    char c;

    memcpy(basfil, tmpdir->chrs,tmpdir->len);
    memcpy(basfil+tmpdir->len,"SFX_BAS", 8);

    if((ch = io_open(basfil, NEW_OVER)) > 0)
    {
        char *p,txt[80];
        int l;

        p = txt;
        *p++ = 'E';
        *p++ = 'W';
        *p++ = ' ';
        memcpy(p, tmpdir->chrs, tmpdir->len);
        p += tmpdir->len;
        memcpy(p, "SFX_EXE;'-d ", 12);
        p += 12;
        memcpy(p, outdir->chrs, outdir->len);
        p += outdir->len;
        *p++ = '\'';
        *p++ = '\n';
        io_sstrg(ch, -1, txt, (int)(p-txt));

        memcpy(txt, "delete ", 7);
        p = txt + 7;
        memcpy(p, tmpdir->chrs, tmpdir->len);
        p += tmpdir->len;
        memcpy(p, "SFX_EXE\n", 8);
        p += 4;
        l = (int)(p+4-txt);
        io_sstrg(ch, -1, txt, l);
        memcpy(p, "DAT\n", 4);
        io_sstrg(ch, -1, txt, l);
        memcpy(p, "BAS\n", 4);
        io_sstrg(ch, -1, txt, l);
        io_close(ch);
        makesfx((__QP_t *)tmpdir);
    }
    else r = ch;

    if(r == 0)
    {
        char t3[80];
        char *p;
        p = t3;
        memcpy(p, T3, T3LEN);
        p += T3LEN;
        memcpy (p, basfil, tmpdir->len+7);
        p += tmpdir->len+7;
        *p++ = '"';
        *p++ = '\n';
        io_sstrg(con, -1, t3, (int)(p-t3));
    }
    io_sstrg(con, -1, T4, T4LEN);
    io_fbyte(con, (5*60*50), &c);
    return r;
}

int main(void)
{
    int con;
    int r,n;
    __QA(PATH_MAX) tmpdir;
    __QA(PATH_MAX) outdir;
    char basfil[PATH_MAX];

    con = io_open("con_480x160a16x38", 0);
    sd_bordr(con, -1, 7, 2);
    sd_clear(con, -1);
    sd_cure (con, -1);

    io_sstrg(con, -1, T1, T1LEN);
    if((tmpdir.len = io_fline(con, -1, tmpdir.chrs, PATH_MAX-1)) > 1)
    {
        if((r = checkdir((__QP_t *)&tmpdir)) == 0)
        {
            io_sstrg(con, -1, T2, T2LEN);
            if((outdir.len = io_fline(con, -1, outdir.chrs, PATH_MAX-1)) > 0)
            {
                if((r = checkdir((__QP_t *)&outdir)) == 0)
                {
                    r = unpackit ((__QP_t *)&tmpdir, (__QP_t *)&outdir,
                                    basfil, con);
                }
            }
        }
    }
    sd_bordr(con, -1, 0, 0);
    sd_clear(con, -1);
    io_close(con);
    return r;
}
