#include <time.h>
#ifdef localtime
#undef localtime
#endif
struct tm *(*foo)(const time_t *) = localtime;
int main(void) { return 0; }
