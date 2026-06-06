/* Test whether a basic scandir invocation works. */

#include <dirent.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

#include "../basic.h"

static int no_sources(const struct dirent* entry)
{
	if ( 2 <= strlen(entry->d_name) &&
	     !strcmp(entry->d_name + strlen(entry->d_name) - 2, ".c") )
		return 0;
	return 1;
}

int main(void)
{
	struct dirent** entries;
	int count = scandir("dirent", &entries, no_sources, alphasort);
	if ( count < 0 )
		err(1, "scandir");
	bool found = false;
	for ( int i = 0; i < count; i++ )
	{
		if ( i && 0 <= strcoll(entries[i-1]->d_name, entries[i]->d_name) )
			errx(1, "scandir gave wrong order: %s vs %s",
			     entries[i-1]->d_name, entries[i]->d_name);
		if ( !no_sources(entries[i]) )
			errx(1, "%s wasn't filtered away", entries[i]->d_name);
		if ( !strcmp(entries[i]->d_name, "scandir") )
			found = true;
	}
	for ( int i = 0; i < count; i++ )
		free(entries[i]);
	free(entries);
	if ( !found )
		errx(1, "did not find scandir program");
	return 0;
}
