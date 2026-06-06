/*[XSI]*/
/* Test whether a basic lfind invocation works. */

#include <search.h>

#include "../basic.h"

static int compare(const void* a_ptr, const void* b_ptr)
{
	return *((int*) a_ptr) - *((int*) b_ptr); 
}

int main(void)
{
	int one = 1;
	int two = 2;
	int three = 3;
	int table[4] = { 1, 2, 1, 0 };
	size_t count = 3;
	void* one_ptr = lfind(&one, table, &count, sizeof(int), compare);
	if ( one_ptr != &table[0] )
		errx(1, "lfind didn't find one at [0]");
	if ( count != 3 )
		errx(1, "first lfind count != 3");
	if ( table[0] != 1 )
		errx(1, "first lfind table[0] != 1");
	if ( table[1] != 2 )
		errx(1, "first lfind table[1] != 2");
	if ( table[2] != 1 )
		errx(1, "first lfind table[2] != 1");
	if ( table[3] != 0 )
		errx(1, "first lfind table[3] != 0");
	void* two_ptr = lfind(&two, table, &count, sizeof(int), compare);
	if ( two_ptr != &table[1] )
		errx(1, "lfind didn't find two at [1]");
	if ( count != 3 )
		errx(1, "second lfind count != 3");
	if ( table[0] != 1 )
		errx(1, "second lfind table[0] != 1");
	if ( table[1] != 2 )
		errx(1, "second lfind table[1] != 2");
	if ( table[2] != 1 )
		errx(1, "second lfind table[2] != 1");
	if ( table[3] != 0 )
		errx(1, "second lfind table[3] != 0");
	void* three_ptr = lfind(&three, table, &count, sizeof(int), compare);
	if ( three_ptr != NULL )
		errx(1, "lfind found absent three");
	if ( count != 3 )
		errx(1, "second lfind count != 3");
	if ( table[0] != 1 )
		errx(1, "second lfind table[0] != 1");
	if ( table[1] != 2 )
		errx(1, "second lfind table[1] != 2");
	if ( table[2] != 1 )
		errx(1, "second lfind table[2] != 1");
	if ( table[3] != 0 )
		errx(1, "second lfind table[3] != 0");
	return 0;
}
