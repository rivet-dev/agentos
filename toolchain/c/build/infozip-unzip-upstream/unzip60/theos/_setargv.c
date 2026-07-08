/*
  Copyright (c) 1990-2001 Info-ZIP.  All rights reserved.

  See the accompanying file LICENSE, version 2000-Apr-09 or later
  (the contents of which are also included in unzip.h) for terms of use.
  If, for some reason, all these files are missing, the Info-ZIP license
  also may be found at:  ftp://ftp.info-zip.org/pub/infozip/license.html
*/
/*
 * _setargv.c - derived from an command argument expander
 *
 * Author  : Jean-Michel Dubois
 * Date    : 13-Dec-98
 *
 * Function: Looks for member names (fn.ft.mb) and add the libraries names
 *           (fn.ft) to the list of files to zip to force inclusion of
 *           libraries if necessary.
 *           Strings beginning by a dash are considered as options and left
 *           unchanged.
 *
 * Syntax  : void _setargv(int *argc, char ***argv);
 *
 * Returns : new argc. Caller's argc and argv are updated.
 *       If a insufficient memory condition occurs, return 0 and errno
 *       is set to ENOMEM.
 *
 * Example :
 *      main(int argc, char **argv)
 *      {
 *          if (_setargv(&argc, &argv)) {
 *              ...
 */
#pragma library

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <malloc.h>
#include <errno.h>
#include <scr.h>
#include <peek.h>

/* Allocate argv array in 16 entries chunks */

static int allocarg(int n, int asize, char ***nargv, char *s)
{
    if ((n+1) > asize) {    /* If array full */
        asize += 16;        /* increase size and reallocate */
        if (!(*nargv = (char **) realloc(*nargv, asize * sizeof (void *)))) {
            errno = _errnum = ENOMEM;    /* Not enough memory */
            return 0;
        }
    }
    (*nargv)[n] = strdup(s);    /* Save argument */
    return asize;               /* Return new maxsize */
}

/* check if file is a member of a library */

static int ismember(char* path)
{
    char* p;

    if ((p = strrchr(path, '/')) == NULL)
        p = path;
    return ((p = strchr(p, '.')) && (p = strchr(p + 1, '.')));
}

/* extract library name from a file name */

static char* libname(char* path)
{
    char* p;
    static char lib[256];
    char disk[3];

    strcpy(lib, path);
    if (p = strrchr(lib, ':')) {
        strncpy(disk, p, 2);
        disk[2] = '\0';
        *p = '\0' ;
    } else
        disk[0] = '\0';

    if ((p = strrchr(lib, '/')) == NULL)
        p = lib;

    p = strchr(p, '.');
    p = strchr(p + 1, '.');
    *p = 0;
    strcat(lib, disk);
    return lib;
}

/* Main body of the function */

int _setargv(int *argc, char ***argv)
{
    register int nargc;     /* New arguments counter */
    char **nargv;           /* New arguments pointers */
    register int i, j;
    int asize;              /* argv array size */
    char *arg;
    char lib[256];

    _errnum = 0;
    nargc = 0;          /* Initialise counter, size counter */
    asize = *argc;      /* and new argument vector to the */
                        /* current argv array size */

    if ((nargv = (char **) calloc((size_t) *argc, sizeof (void *))) != NULL) {
        /* For each initial argument */
        for (i = 0; i < *argc; i++) {
            arg = (*argv)[i];
#ifdef DEBUG
            fprintf(stderr, "checking arg: %s", arg);
#endif
            if (i == 0 || *arg == '-' || ! ismember(arg)) {
                /* if it begins with a dash or doesn't include
                 * a library name simply add it to the new array */
                if (! (asize = allocarg(nargc, asize, &nargv, arg)))
                    return 0;   /* Not enough memory */
                nargc++;
            } else {
                short insert;
                strcpy(lib, libname(arg));
                /* add library name if necessary */
                for (j = 2, insert = 1; i < nargc; i++) {
                    if (ismember(nargv[i])
                     && ! strcmp(lib, libname(nargv[i]))) {
                        insert = 0;
                        break;
                    }
                }
                if (insert) {
#ifdef DEBUG
                    fprintf(stderr, "inserting lib %s ", lib);
#endif
                    if (! (asize = allocarg(nargc, asize, &nargv, lib)))
                        return 0;   /* Not enough memory */
                    nargc++;
                }
                /* add file name */
#ifdef DEBUG
                fprintf(stderr, "inserting file %s", arg);
#endif
                if (! (asize = allocarg(nargc, asize, &nargv, arg)))
                    return 0;   /* Not enough memory */
                nargc++;
            }
#ifdef DEBUG
            fprintf(stderr, "\n");
#endif
        }
        /* Update caller's parameters */
        *argc = nargc;
        *argv = nargv;
        /* and sign on success */
        return nargc;
    }

    /* If it is not possible to allocate initial array, sign on error */
    _errnum = ENOMEM;
    return 0;
}
