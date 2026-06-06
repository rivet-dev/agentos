/*[XSI]*/
/* Test whether a basic lsearch invocation works. */

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
	int new_one = 1;
	int table[3] = { 0, 0, 0 };
	size_t count = 0;
	void* one_ptr = lsearch(&one, table, &count, sizeof(int), compare);
	if ( one_ptr != &table[0] )
		errx(1, "lsearch didn't insert one at [0]");
	if ( count != 1 )
		errx(1, "first lsearch count != 1");
	if ( table[0] != 1 )
		errx(1, "first lsearch table[0] != 1");
	if ( table[1] != 0 )
		errx(1, "first lsearch table[1] != 0");
	if ( table[2] != 0 )
		errx(1, "first lsearch table[2] != 0");
	void* two_ptr = lsearch(&two, table, &count, sizeof(int), compare);
	if ( two_ptr != &table[1] )
		errx(1, "lsearch didn't insert two at [1]");
	if ( count != 2 )
		errx(1, "second lsearch count != 2");
	if ( table[0] != 1 )
		errx(1, "second lsearch table[0] != 1");
	if ( table[1] != 2 )
		errx(1, "second lsearch table[1] != 2");
	if ( table[2] != 0 )
		errx(1, "second lsearch table[2] != 0");
	void* new_one_ptr = lsearch(&new_one, table, &count, sizeof(int), compare);
	if ( new_one_ptr != &table[0] )
		errx(1, "lsearch didn't find new_one at [0]");
	if ( count != 2 )
		errx(1, "third lsearch count != 2");
	if ( table[0] != 1 )
		errx(1, "third lsearch table[0] != 1");
	if ( table[1] != 2 )
		errx(1, "third lsearch table[1] != 2");
	if ( table[2] != 0 )
		errx(1, "third lsearch table[2] != 0");
	return 0;
}
