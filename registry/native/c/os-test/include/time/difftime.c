#include <time.h>
#ifdef difftime
#undef difftime
#endif
double (*foo)(time_t, time_t) = difftime;
int main(void) { return 0; }
