/*[OB]*/
#include <time.h>
#ifdef asctime
#undef asctime
#endif
char *(*foo)(const struct tm *) = asctime;
int main(void) { return 0; }
