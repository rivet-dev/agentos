#include <net/if.h>
#ifdef if_indextoname
#undef if_indextoname
#endif
char *(*foo)(unsigned, char *) = if_indextoname;
int main(void) { return 0; }
