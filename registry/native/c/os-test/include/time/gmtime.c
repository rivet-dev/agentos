#include <time.h>
#ifdef gmtime
#undef gmtime
#endif
struct tm *(*foo)(const time_t *) = gmtime;
int main(void) { return 0; }
