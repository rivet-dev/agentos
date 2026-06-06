/* Test whether a basic strtoimax invocation works. */

#include <inttypes.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char* end;
	intmax_t value = strtoimax("-4611686014132420609.1end", &end, 10);
	intmax_t expected = INTMAX_C(-4611686014132420609);
	if ( value != expected )
		errx(1, "strtoimax returned %"PRIdMAX" rather than %"PRIdMAX"",
		     value, expected);
	if ( strcmp(end, ".1end") != 0 )
		errx(1, "strtoimax set wrong end pointer");
	return 0;
}
