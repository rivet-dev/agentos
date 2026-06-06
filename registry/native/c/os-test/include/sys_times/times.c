#include <sys/times.h>
#ifdef times
#undef times
#endif
clock_t (*foo)(struct tms *) = times;
int main(void) { return 0; }
