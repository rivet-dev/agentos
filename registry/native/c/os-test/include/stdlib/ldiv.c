#include <stdlib.h>
#ifdef ldiv
#undef ldiv
#endif
ldiv_t (*foo)(long, long) = ldiv;
int main(void) { return 0; }
