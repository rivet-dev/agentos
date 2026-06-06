/*[OB]*/
#include <time.h>
#ifdef ctime
#undef ctime
#endif
char *(*foo)(const time_t *) = ctime;
int main(void) { return 0; }
