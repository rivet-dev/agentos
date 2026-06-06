/*[XSI]*/
/* Test whether a basic tsearch invocation works. */

#include <search.h>
#include <string.h>

#include "../basic.h"

typedef void tnode; // posix_tnode is not universal yet

static int compare(const void* a, const void* b)
{
	return strcmp((const char*) a, (const char*) b);
}

int main(void)
{
	const char* foo_string = "foo";
	const char* bar_string = "bar";
	const char* foo_again_string = strdup(foo_string);
	if ( !foo_again_string )
		err(1, "malloc");
	tnode* root = NULL;
	// Insert foo.
	tnode* foo_node = tsearch(foo_string, &root, compare);
	if ( !foo_node )
		errx(1, "tsearch foo failed");
	if ( root != foo_node )
		errx(1, "root != foo_node");
	if ( *(const char**) foo_node != foo_string )
		errx(1, "*(const char**) foo_node != foo_string");
	// Insert bar.
	tnode* bar_node = tsearch(bar_string, &root, compare);
	if ( !bar_node )
		errx(1, "tsearch bar failed");
	if ( root != foo_node && root != bar_node )
		errx(1, "root != foo_node && root != bar_node");
	if ( *(const char**) bar_node != bar_string )
		errx(1, "*(const char**) bar_node != bar_string");
	// Try reinsert foo (and find the existing foo).
	tnode* foo_again = tsearch(foo_again_string, &root, compare);
	if ( !foo_again )
		errx(1, "tsearch foo again failed");
	if ( foo_again != foo_node )
		errx(1, "tsearch did not find the same foo node");
	if ( *(const char**) foo_again != foo_string )
		errx(1, "*(const char**) foo_again != foo_string");
	if ( *(const char**) foo_again == foo_again_string )
		errx(1, "*(const char**) foo_again == foo_again_string");
	return 0;
}
