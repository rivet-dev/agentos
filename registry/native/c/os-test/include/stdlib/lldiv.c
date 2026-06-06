#include <stdlib.h>
#ifdef lldiv
#undef lldiv
#endif
lldiv_t (*foo)(long long, long long) = lldiv;
int main(void) { return 0; }
