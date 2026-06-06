/* Test whether a basic mkdtemp invocation works. */

#include <stdlib.h>
#include <unistd.h>

#include "../basic.h"

int main(void)
{
	const char* tmpdir = getenv("TMPDIR");
	if ( !tmpdir )
		tmpdir = "/tmp";
	size_t template_len = strlen(tmpdir) + strlen("/os-test.XXXXXX");
	char* template = malloc(template_len + 1);
	if ( !template )
		err(1, "malloc");
	strcpy(template, tmpdir);
	strcat(template, "/os-test.XXXXXX");
	char* result = mkdtemp(template);
	if ( !result )
		err(1, "mkdtemp");
	if ( result != template )
	{
		warn("mkdtemp did not return template");
		rmdir(template);
		rmdir(result);
		exit(1);
	}
	rmdir(template);
	return 0;
}
