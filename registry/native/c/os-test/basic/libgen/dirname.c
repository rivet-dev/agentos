/*[XSI]*/
/* Test whether a basic dirname invocation works. */

#include <libgen.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char input1[] = "foo";
	char copy1[] = "foo";
	char expected1[] = ".";
	char* output1 = dirname(input1);
	if ( strcmp(output1, expected1) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy1, output1, expected1);

	char input2[] = "foo/";
	char copy2[] = "foo/";
	char expected2[] = ".";
	char* output2 = dirname(input2);
	if ( strcmp(output2, expected2) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy2, output2, expected2);

	char input3[] = "/foo/";
	char copy3[] = "/foo/";
	char expected3[] = "/";
	char* output3 = dirname(input3);
	if ( strcmp(output3, expected3) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy3, output3, expected3);

	char input4[] = "/foo//bar//";
	char copy4[] = "/foo//bar//";
	char expected4[] = "/foo";
	char* output4 = dirname(input4);
	if ( strcmp(output4, expected4) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy4, output4, expected4);

	char input5[] = "//foo//bar//";
	char copy5[] = "//foo//bar//";
	char expected5a[] = "/foo";
	char expected5b[] = "//foo";
	char* output5 = dirname(input5);
	if ( strcmp(output5, expected5a) != 0 &&
	     strcmp(output5, expected5b) != 0 )
		errx(1, "basename(\"%s\") was \"%s\" not \"%s\" or \"%s\"",
		     copy5, output5, expected5a, expected5b);

	char input6[] = "/foo/bar/.";
	char copy6[] = "/foo/bar/.";
	char expected6[] = "/foo/bar";
	char* output6 = dirname(input6);
	if ( strcmp(output6, expected6) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy6, output6, expected6);

	char input7[] = "/foo/../bar";
	char copy7[] = "/foo/../bar";
	char expected7[] = "/foo/..";
	char* output7 = dirname(input7);
	if ( strcmp(output7, expected7) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy7, output7, expected7);

	char input8[] = "/foo/bar/qux";
	char copy8[] = "/foo/bar/qux";
	char expected8[] = "/foo/bar";
	char* output8 = dirname(input8);
	if ( strcmp(output8, expected8) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy8, output8, expected8);

	char input9[] = "/";
	char copy9[] = "/";
	char expected9[] = "/";
	char* output9 = dirname(input9);
	if ( strcmp(output9, expected9) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy9, output9, expected9);

	char input10[] = "//";
	char copy10[] = "//";
	char expected10a[] = "/";
	char expected10b[] = "//";
	char* output10 = dirname(input10);
	if ( strcmp(output10, expected10a) != 0 &&
	     strcmp(output10, expected10b) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\" or \"%s\"",
		     copy10, output10, expected10a, expected10b);

	char input11[] = "///";
	char copy11[] = "///";
	char expected11[] = "/";
	char* output11 = dirname(input11);
	if ( strcmp(output11, expected11) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy11, output11, expected11);

	char input12[] = "";
	char copy12[] = "";
	char expected12[] = ".";
	char* output12 = dirname(input12);
	if ( strcmp(output12, expected12) != 0 )
		errx(1, "dirname(\"%s\") was \"%s\" not \"%s\"",
		     copy12, output12, expected12);

	char* input13 = NULL;
	char expected13[] = ".";
	char* output13 = dirname(input13);
	if ( strcmp(output13, expected13) != 0 )
		errx(1, "dirname(NULL) was \"%s\" not \"%s\"",
		     output13, expected13);

	return 0;
}
