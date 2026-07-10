/*
  Copyright (c) 1990-2000 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*
 * makesfx - Makes a QDOS sfx zip file
 * ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
 * created by Jonathan Hudson, 04/09/95
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>

#define SFXFLAG "??Special Flag for unzipsfx hack  ??"

#ifdef QDOS
# include <qdos.h>
# define ZMODE (X_OK|R_OK)
# define rev_long(x) (x)
# define XFLAG 0x4afb
#else
# define ZMODE (R_OK)
# define getchid(p1) p1
# include <sys/stat.h>
  long rev_long(long l);
# define XFLAG 0xfb4a

typedef struct
{
    long id;
    long dlen;
} NTC;

struct qdirect  {
    long            d_length __attribute__ ((packed));  /* file length */
    unsigned char   d_access __attribute__ ((packed));  /* file access type */
    unsigned char   d_type __attribute__ ((packed));    /* file type */
    long            d_datalen __attribute__ ((packed)); /* data length */
    long            d_reserved __attribute__ ((packed));/* Unused */
    short           d_szname __attribute__ ((packed));  /* size of name */
    char            d_name[36] __attribute__ ((packed));/* name area */
    long            d_update __attribute__ ((packed));  /* last update */
    long            d_refdate __attribute__ ((packed));
    long            d_backup __attribute__ ((packed));   /* EOD */
} ;

int fs_headr (int fd, long t, struct qdirect *qs, short size)
{
    NTC ntc;
    int r = -1;
    struct stat s;

    fstat(fd, &s);
    qs->d_length = s.st_size;
    lseek(fd, -8, SEEK_END);
    read(fd, &ntc, 8);
    if(ntc.id == *(long *)"XTcc")
    {
        qs->d_datalen = ntc.dlen;    /* This is big endian */
        qs->d_type = 1;
        r = 0;
    }
    lseek(fd, 0, 0);
    return 42;                       /* why not ??? */
}

typedef unsigned char uch;

long rev_long (long l)
{
    uch cc[4];
    cc[0] = (uch)(l >> 24);
    cc[1] = (uch)((l >> 16) & 0xff);
    cc[2] = (uch)((l >> 8) & 0xff);
    cc[3] = (uch)(l & 0xff);
    return *(long *)cc;
}

#endif

#define RBUFSIZ 4096

void usage(void)
{
    fputs("makesfx -o outfile -z zipfile -xunzipsfx -sstubfile\n", stderr);
    exit(0);
}

int main (int ac, char **av)
{
    int fd, fo;
    static char local_sig[4] = "PK\003\004";
    char *p, tmp[4];
    short ok = 0;
    char *of = NULL;
    char *xf = NULL;
    char *zf = NULL;
    char *sf = NULL;
    int c;

    while((c = getopt(ac, av, "o:z:x:s:h")) != EOF)
    {
        switch(c)
        {
            case 'o':
                of = optarg;
                break;
            case 'z':
                zf = optarg;
                break;
            case 'x':
                xf = optarg;
                break;
            case 's':
                sf = optarg;
                break;
            case 'h':
                usage();
                break;
        }
    }


    if(zf && xf && of && sf)
    {
        if((fd = open(zf, O_RDONLY)) > 0)
        {
            if((read(fd, tmp, 4) == 4))
            {
                if(*(long *)tmp == *(long *)local_sig)
                {
                    ok = 1;
                }
            }
            close(fd);
        }
        if(!ok)
        {
            fprintf(stderr,
                    "Huum, %s doesn't look like a ZIP file to me\n", zf);
            exit(0);
        }

        if(strstr(xf, "unzipsfx"))
        {
            if(access(xf, ZMODE))
            {
                fprintf(stderr, "Sorry, don't like the look of %s\n", xf);
                exit(0);
            }
        }

        if((fo = open(of, O_CREAT|O_TRUNC|O_RDWR, 0666)) != -1)
        {
            struct qdirect sd,xd;
            int n;
            int dsoff = 0;
            int nfoff = 0;
            int zlen = 0;

            if((fd = open(sf, O_RDONLY)) != -1)
            {
                if(fs_headr(getchid(fd), -1, &sd, sizeof(sd)) > 0)
                {
                    unsigned short *q;
                    p = malloc(sd.d_length);
                    n = read(fd, p, sd.d_length);
                    for(q = (unsigned short *)p;
                        q != (unsigned short *)(p+sd.d_length); q++)
                    {
                        if(*q == XFLAG && *(q+1) == XFLAG)
                        {
                            dsoff = (int)q-(int)p;
                            break;
                        }
                    }
                    write(fo, p, n);
                    close(fd);
                }
            }

            if(dsoff == 0)
            {
                puts("Fails");

                exit(0);
            }

            if((fd = open(xf, O_RDONLY)) != -1)
            {
                char *q;
                if(fs_headr(getchid(fd), -1, &xd, sizeof(xd)) > 0)
                {
                    p = realloc(p, xd.d_length);
                    n = read(fd, p, xd.d_length);
                    {
                        for(q = p; q < p+xd.d_length ; q++)
                        {
                            if(*q == '?')
                            {
                                if(memcmp(q, SFXFLAG, sizeof(SFXFLAG)-1) == 0)
                                {
                                    nfoff = (int)(q-p);
                                    break;
                                }
                            }
                        }
                    }
                    write(fo, p, n);
                    close(fd);

                    if((fd = open(zf, O_RDONLY)) > 0)
                    {
                        p = realloc(p, RBUFSIZ);
                        while((n = read(fd, p, RBUFSIZ)) > 0)
                        {
                            write(fo, p, n);
                            zlen += n;
                        }
                        close(fd);
                    }
                    lseek(fo, dsoff+4, SEEK_SET);
                    n = rev_long((sd.d_length-dsoff));
                    write(fo, &n, sizeof(long));
                    n = rev_long(xd.d_length);
                    write(fo, &n, sizeof(long));
                    write(fo, &xd.d_datalen, sizeof(long));
                    n = rev_long(nfoff);
                    write(fo, &n, sizeof(long));
                    n = rev_long(zlen);
                    write(fo, &n, sizeof(long));
                    close(fo);
                }
                else
                {
                    close(fd);
                    fputs("Can't read unzipsfx header", stderr);
                    exit(0);
                }
            }
            free(p);
        }
    }
    else
        usage();

    return 0;
}
