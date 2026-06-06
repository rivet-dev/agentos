/*[XSI]*/
/* Test whether a basic nftw invocation works. */

#include <sys/stat.h>

#include <ftw.h>
#include <stdbool.h>

#include "../basic.h"

bool found_nftw = false;
bool found_ftw = false;
bool found_dot = false;

static int iterator(const char* path, const struct stat* st, int type,
                    struct FTW* ftw)
{
	(void) st;
	(void) type;
	(void) ftw;
	if ( !strcmp(path, "./ftw/nftw") )
	{
		if ( type != FTW_F )
			errx(1, "./ftw/nftw was not FTW_F");
		if ( !S_ISREG(st->st_mode) )
			errx(1, "./ftw/nftw was not regular file");
		if ( strcmp(path + ftw->base, "nftw") != 0 )
			errx(1, "./ftw/nftw basename was not nftw");
		if ( ftw->level != 2 )
			errx(1, "./ftw/nftw level was not 2");
		if ( found_nftw )
			errx(1, "found ./ftw/nftw twice");
		found_nftw = true;
	}
	else if ( !strcmp(path, "./ftw") )
	{
		if ( type != FTW_DP )
			errx(1, "./ftw was not FTW_DP");
		if ( !S_ISDIR(st->st_mode) )
			errx(1, "./ftw was not directory");
		if ( strcmp(path + ftw->base, "ftw") != 0 )
			errx(1, "./ftw basename was not ftw");
		if ( ftw->level != 1 )
			errx(1, "./ftw level was not 1");
		if ( found_ftw )
			errx(1, "found ./ftw twice");
		if ( !found_nftw )
			errx(1, "found ./ftw before ./ftw/nftw");
		found_ftw = true;
	}
	else if ( !strcmp(path, ".") )
	{
		if ( type != FTW_DP )
			errx(1, ". was not FTW_DP");
		if ( !S_ISDIR(st->st_mode) )
			errx(1, ". was not directory");
		if ( strcmp(path + ftw->base, ".") != 0 )
			errx(1, ". basename was not .");
		if ( ftw->level != 0 )
			errx(1, ". level was not 0");
		if ( found_dot )
			errx(1, "found ./ftw twice");
		if ( !found_nftw )
			errx(1, "found . before ./ftw/nftw");
		if ( !found_ftw )
			errx(1, "found . before ./ftw");
		found_dot = true;
	}
	return 0;
}

int main(void)
{
	if ( nftw(".", iterator, 1024, FTW_DEPTH) < 0 )
		err(1, "nftw");
	if ( !found_dot || !found_ftw || !found_nftw )
		errx(1, "nftw did not find files and directories");
	return 0;
}
