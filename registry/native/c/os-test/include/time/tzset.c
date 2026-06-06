#include <time.h>
#ifdef tzset
#undef tzset
#endif
void (*foo)(void) = tzset;
int main(void) { return 0; }
