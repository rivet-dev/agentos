/*[XSI]*/
/* Test whether a basic twalk invocation works. */

#include <search.h>
#include <string.h>

#include "../basic.h"

typedef void tnode; // posix_tnode is not universal yet

static int compare(const void* a, const void* b)
{
	return strcmp((const char*) a, (const char*) b);
}

static tnode* foo_node = NULL;
static tnode* bar_node = NULL;
static tnode* root = NULL;
static int state = 0;

static void walk(const tnode* node, VISIT visit, int depth)
{
	switch ( ++state )
	{
	case 1:
		if ( node != root )
			errx(1, "first walk: node != root");
		if ( visit != preorder )
			errx(1, "first walk: visit != preorder");
		if ( depth != 0 )
			errx(1, "first walk: depth != 0");
		break;
	case 2:
		if ( root == foo_node )
		{
			if ( node != bar_node )
				errx(1, "second walk: node != bar_node");
			if ( visit != leaf )
				errx(1, "second walk: visit != leaf");
			if ( depth != 1 )
				errx(1, "second walk: depth != 1");
		}
		else
		{
			if ( node != bar_node )
				errx(1, "second walk: node != bar_node");
			if ( visit != postorder )
				errx(1, "second walk: visit != postorder");
			if ( depth != 0 )
				errx(1, "second walk: depth != 0");
		}
		break;
	case 3:
		if ( root == foo_node )
		{
			if ( node != foo_node )
				errx(1, "third walk: node != bar_node");
			if ( visit != postorder )
				errx(1, "third walk: visit != postorder");
			if ( depth != 0 )
				errx(1, "third walk: depth != 0");
		}
		else
		{
			if ( node != foo_node )
				errx(1, "third walk: node != bar_node");
			if ( visit != leaf )
				errx(1, "third walk: visit != leaf");
			if ( depth != 1 )
				errx(1, "third walk: depth != 1");
		}
		break;
	case 4:
		if ( node != root )
			errx(1, "fourth walk: node != root");
		if ( visit != endorder )
			errx(1, "fourth walk: visit != endorder");
		if ( depth != 0 )
			errx(1, "fourth walk: depth != 0");
		break;
	case 5:
		errx(1, "fifth walk should not happen");
	}
}

int main(void)
{
	// Insert foo and bar.
	foo_node = tsearch("foo", &root, compare);
	if ( !foo_node )
		errx(1, "tsearch foo failed");
	bar_node = tsearch("bar", &root, compare);
	if ( !bar_node )
		errx(1, "tsearch bar failed");
	// Do the walk.
	twalk(root, walk);
	// Do the empty walk.
	tnode* empty = NULL;
	twalk(empty, walk);
	return 0;
}
