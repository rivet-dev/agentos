/* Test whether a basic fwide invocation works. */

#include <wchar.h>

#include "../basic.h"

int main(void)
{
	if ( fwide(stdin, 0) != 0 )
		errx(1, "stdin had orientation");
	if ( fwide(stdout, 0) != 0 )
		errx(1, "stdin had orientation");
	if ( fwide(stderr, 0) != 0 )
		errx(1, "stdin had orientation");
	FILE* fp1 = tmpfile();
	FILE* fp2 = tmpfile();
	FILE* fp3 = tmpfile();
	FILE* fp4 = tmpfile();
	if ( !fp1 || !fp2 || !fp3 || !fp4 )
		err(1, "tmpfile");
	if ( fwide(fp1, 0) != 0 )
		errx(1, "tmpfile had orientation");
	// Test fputc setting byte orientation.
	if ( fputc('x', fp1) == EOF )
		errx(1, "fputc");
	if ( 0 <= fwide(fp1, 0) )
		errx(1, "fputc did not set byte orientation");
	// Test fgetc setting byte orientation.
	if ( fgetc(fp2) != EOF )
		errx(1, "fputc");
	if ( 0 <= fwide(fp2, 0) )
		errx(1, "fgetc did not set byte orientation");
	// Test fputwc setting wide orientation.
	if ( fputwc(L'x', fp3) == WEOF )
		errx(1, "fputwc");
	if ( fwide(fp3, 0) <= 0 )
		errx(1, "fputc did not set wide orientation");
	// Test fgetwc setting wide orientation.
	if ( fgetwc(fp4) != WEOF )
		errx(1, "fgetwc");
	if ( fwide(fp4, 0) <= 0 )
		errx(1, "fgetc did not set wide orientation");
	return 0;
}
