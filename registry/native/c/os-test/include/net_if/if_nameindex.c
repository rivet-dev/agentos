#include <net/if.h>
#ifdef if_nameindex
#undef if_nameindex
#endif
struct if_nameindex *(*foo)(void) = if_nameindex;
int main(void) { return 0; }
