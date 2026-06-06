/* Test whether a basic inet_ntop invocation works. */

#include <sys/socket.h>

#include <arpa/inet.h>
#include <errno.h>
#include <string.h>

#include "../basic.h"

int main(void)
{
	char ip_buf[INET_ADDRSTRLEN];
	const char* expected;

	unsigned char ip_1[4] = {0x01, 0x02, 0x03, 0x04};
	expected = "1.2.3.4";
	if ( inet_ntop(AF_INET, ip_1, ip_buf, sizeof(ip_buf)) != ip_buf )
		errx(1, "inet_ntop %s failed", expected);
	if ( strcmp(ip_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip_buf, expected);

	unsigned char ip_2[4] = {0xFF, 0xFF, 0xFF, 0xFF};
	expected = "255.255.255.255";
	if ( inet_ntop(AF_INET, ip_2, ip_buf, sizeof(ip_buf)) != ip_buf )
		errx(1, "inet_ntop %s failed", expected);
	if ( strcmp(ip_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip_buf, expected);

	// Enforce RFC 5952 for IPv6 addresses, even though it is techically not
	// required by POSIX since implementations follow it in practice, and these
	// semantics are important for the reasons in the RFC and should honestly be
	// standardized.
#if defined(AF_INET6) && defined(INET6_ADDRSTRLEN)
	// Test leading zeros are not included and letters are lowercase.
	char ip6_buf[INET6_ADDRSTRLEN];
	expected = "123:4567:89ab:cdef:cafe:babe:dead:beef";
	unsigned char ip6_1[16] = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
	                           0xca, 0xfe, 0xba, 0xbe, 0xde, 0xad, 0xbe, 0xef};
	if ( inet_ntop(AF_INET6, ip6_1, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test the leftmost :: is preferred if two sequences have the same length.
	expected = "123::cdef:cafe:0:0:beef";
	unsigned char ip6_2[16] = {0x01, 0x23, 0x00, 0x00, 0x00, 0x00, 0xcd, 0xef,
	                           0xca, 0xfe, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( inet_ntop(AF_INET6, ip6_2, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test the leftmost :: is preferred if two sequences have the same length,
	// also for the leading sequence. This case is a bug in e.g. musl.
	expected = "::89ab:cdef:cafe:0:0:beef";
	unsigned char ip6_3[16] = {0x00, 0x00, 0x00, 0x00, 0x89, 0xab, 0xcd, 0xef,
	                           0xca, 0xfe, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( inet_ntop(AF_INET6, ip6_3, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test the longest :: sequence is preferred.
	expected = "123:0:0:cdef::beef";
	unsigned char ip6_4[16] = {0x01, 0x23, 0x00, 0x00, 0x00, 0x00, 0xcd, 0xef,
	                           0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xbe, 0xef};
	if ( inet_ntop(AF_INET6, ip6_4, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test :: is not used to shorten a single field.
	expected = "123:4567:89ab:cdef:0:babe:dead:beef";
	unsigned char ip6_5[16] = {0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
	                           0x00, 0x00, 0xba, 0xbe, 0xde, 0xad, 0xbe, 0xef};
	if ( inet_ntop(AF_INET6, ip6_5, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test shorting everything to :: for the any address.
	expected = "::";
	unsigned char ip6_6[16] = {0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	                           0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00};
	if ( inet_ntop(AF_INET6, ip6_6, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);

	// Test an IPv4-mapped IPv6 address.
	expected = "::ffff:1.2.3.4";
	unsigned char ip6_7[16] = {0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
	                           0x00, 0x00, 0xff, 0xff, 0x01, 0x02, 0x03, 0x04};
	if ( inet_ntop(AF_INET6, ip6_7, ip6_buf, sizeof(ip6_buf)) != ip6_buf )
	{
		if ( errno != EAFNOSUPPORT )
			err(1, "inet_ntop %s failed", expected);
	}
	else if ( strcmp(ip6_buf, expected) != 0 )
		errx(1, "inet_ntop() = %s not %s", ip6_buf, expected);
#endif

	return 0;
}
