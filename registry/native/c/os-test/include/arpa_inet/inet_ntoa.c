/*[OB]*/
#include <arpa/inet.h>
#ifdef inet_ntoa
#undef inet_ntoa
#endif
char *(*foo)(struct in_addr) = inet_ntoa;
int main(void) { return 0; }
