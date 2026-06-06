/* Test whether a basic mktime invocation works. */

#include <stdint.h>
#include <time.h>

#include "../basic.h"

int main(void)
{
	time_t now;
	if ( time(&now) < 0 )
		err(1, "time");
	struct tm tm;
	if ( !localtime_r(&now, &tm) )
		err(1, "localtime_r");
	time_t time = mktime(&tm);
	if ( time == (time_t) -1 )
		err(1, "mktime");
	if ( time != now )
		errx(1, "mktime returned %jd not %jd", (intmax_t) time, (intmax_t) now);
	return 0;
}
