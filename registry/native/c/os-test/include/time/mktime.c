#include <time.h>
#ifdef mktime
#undef mktime
#endif
time_t (*foo)(struct tm *) = mktime;
int main(void) { return 0; }
