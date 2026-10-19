// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title MarketSettlement
 * @notice On-chain settlement and escrow contract for Xavier Marketplace (MKT-21).
 * @dev Owner decision: In v1, balances run off-chain using non-transferable internal credits.
 * On-chain settlement via this contract is designed for Amoy testnet evaluation and future
 * activation following the canonical-token ADR, legal opinion, and 4 weeks on Amoy + audit.
 * 
 * Supports ETH/POL native asset or ERC20 escrow with:
 * - Escrow deposit by buyer with timeout
 * - Completion and release to seller (with optional platform fee split)
 * - Refund to buyer on seller approval or timeout expiry
 * - Mutual cancellation or arbitrator/operator dispute resolution
 * - Batch Merkle root anchoring for verifiable off-chain receipts and listings
 */
contract MarketSettlement {
    enum EscrowState {
        None,
        Deposited,
        Completed,
        Refunded,
        Disputed
    }

    struct EscrowOrder {
        bytes32 orderId;
        address buyer;
        address seller;
        address token; // address(0) for native currency (ETH / POL)
        uint256 amount;
        uint256 feeBps; // platform fee in basis points (e.g. 250 = 2.5%)
        uint256 createdAt;
        uint256 timeoutDuration; // in seconds
        EscrowState state;
        bytes32 listingId;
    }

    address public owner;
    address public feeRecipient;
    uint256 public constant MAX_FEE_BPS = 1000; // 10% maximum fee cap

    // Mapping from orderId to EscrowOrder
    mapping(bytes32 => EscrowOrder) public escrows;

    // Anchored merkle roots: merkleRoot => timestamp anchored
    mapping(bytes32 => uint256) public anchoredRoots;

    event EscrowDeposited(
        bytes32 indexed orderId,
        bytes32 indexed listingId,
        address indexed buyer,
        address seller,
        address token,
        uint256 amount,
        uint256 feeBps,
        uint256 timeoutDuration
    );

    event EscrowCompleted(
        bytes32 indexed orderId,
        address indexed seller,
        uint256 sellerAmount,
        uint256 feeAmount
    );

    event EscrowRefunded(
        bytes32 indexed orderId,
        address indexed buyer,
        uint256 amount,
        string reason
    );

    event EscrowDisputed(
        bytes32 indexed orderId,
        address indexed caller,
        string reason
    );

    event MerkleRootAnchored(
        bytes32 indexed root,
        uint256 leafCount,
        uint256 timestamp
    );

    event FeeRecipientUpdated(address indexed previousRecipient, address indexed newRecipient);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    modifier onlyOwner() {
        require(msg.sender == owner, "Only owner");
        _;
    }

    modifier onlyOrderParties(bytes32 orderId) {
        EscrowOrder storage order = escrows[orderId];
        require(
            msg.sender == order.buyer || msg.sender == order.seller || msg.sender == owner,
            "Not authorized for order"
        );
        _;
    }

    constructor(address _feeRecipient) {
        owner = msg.sender;
        feeRecipient = _feeRecipient == address(0) ? msg.sender : _feeRecipient;
    }

    function transferOwnership(address newOwner) external onlyOwner {
        require(newOwner != address(0), "Zero address");
        emit OwnershipTransferred(owner, newOwner);
        owner = newOwner;
    }

    function setFeeRecipient(address newRecipient) external onlyOwner {
        require(newRecipient != address(0), "Zero address");
        emit FeeRecipientUpdated(feeRecipient, newRecipient);
        feeRecipient = newRecipient;
    }

    /**
     * @notice Deposit funds into escrow for a specific marketplace listing.
     * @param orderId Unique 32-byte identifier for the escrow order.
     * @param listingId Identifier of the marketplace listing.
     * @param seller Recipient of funds upon successful completion.
     * @param token Payment token (address(0) for native ETH/POL).
     * @param amount Amount to escrow.
     * @param feeBps Platform fee in basis points (max 1000).
     * @param timeoutDuration Expiration window in seconds before buyer can claim refund.
     */
    function createEscrow(
        bytes32 orderId,
        bytes32 listingId,
        address seller,
        address token,
        uint256 amount,
        uint256 feeBps,
        uint256 timeoutDuration
    ) external payable {
        require(orderId != bytes32(0), "Invalid order id");
        require(seller != address(0), "Invalid seller address");
        require(seller != msg.sender, "Buyer cannot be seller");
        require(amount > 0, "Amount must be positive");
        require(feeBps <= MAX_FEE_BPS, "Fee exceeds maximum");
        require(timeoutDuration >= 1 hours, "Timeout must be at least 1 hour");
        require(escrows[orderId].state == EscrowState.None, "Order already exists");

        if (token == address(0)) {
            require(msg.value == amount, "Native payment mismatch");
        } else {
            require(msg.value == 0, "Native payment not accepted for token");
            // Transfer ERC20 tokens to this contract
            (bool success, bytes memory data) = token.call(
                abi.encodeWithSignature("transferFrom(address,address,uint256)", msg.sender, address(this), amount)
            );
            require(success && (data.len() == 0 || abi.decode(data, (bool))), "Token transferFrom failed");
        }

        escrows[orderId] = EscrowOrder({
            orderId: orderId,
            buyer: msg.sender,
            seller: seller,
            token: token,
            amount: amount,
            feeBps: feeBps,
            createdAt: block.timestamp,
            timeoutDuration: timeoutDuration,
            state: EscrowState.Deposited,
            listingId: listingId
        });

        emit EscrowDeposited(
            orderId,
            listingId,
            msg.sender,
            seller,
            token,
            amount,
            feeBps,
            timeoutDuration
        );
    }

    /**
     * @notice Release escrowed funds to seller and platform fee recipient.
     * @dev Can be called by buyer or owner (e.g. upon IVN or delivery verification).
     */
    function completeEscrow(bytes32 orderId) external {
        EscrowOrder storage order = escrows[orderId];
        require(order.state == EscrowState.Deposited, "Order not in deposited state");
        require(msg.sender == order.buyer || msg.sender == owner, "Only buyer or owner can release");

        order.state = EscrowState.Completed;

        uint256 feeAmount = (order.amount * order.feeBps) / 10000;
        uint256 sellerAmount = order.amount - feeAmount;

        _payout(order.token, order.seller, sellerAmount);
        if (feeAmount > 0) {
            _payout(order.token, feeRecipient, feeAmount);
        }

        emit EscrowCompleted(orderId, order.seller, sellerAmount, feeAmount);
    }

    /**
     * @notice Refund escrowed funds back to the buyer.
     * @dev Allowed if caller is seller, or owner, or if buyer calls after timeout has expired.
     */
    function refundEscrow(bytes32 orderId, string calldata reason) external {
        EscrowOrder storage order = escrows[orderId];
        require(
            order.state == EscrowState.Deposited || order.state == EscrowState.Disputed,
            "Order not eligible for refund"
        );

        if (msg.sender == order.buyer) {
            require(block.timestamp >= order.createdAt + order.timeoutDuration, "Timeout not expired");
        } else {
            require(msg.sender == order.seller || msg.sender == owner, "Not authorized to refund");
        }

        order.state = EscrowState.Refunded;
        _payout(order.token, order.buyer, order.amount);

        emit EscrowRefunded(orderId, order.buyer, order.amount, reason);
    }

    /**
     * @notice Flag order as disputed, pausing automatic timeouts and payouts.
     */
    function disputeEscrow(bytes32 orderId, string calldata reason) external onlyOrderParties(orderId) {
        EscrowOrder storage order = escrows[orderId];
        require(order.state == EscrowState.Deposited, "Order not in deposited state");

        order.state = EscrowState.Disputed;
        emit EscrowDisputed(orderId, msg.sender, reason);
    }

    /**
     * @notice Owner resolution of a disputed escrow.
     */
    function resolveDispute(
        bytes32 orderId,
        uint256 buyerShareBps,
        string calldata resolutionNotes
    ) external onlyOwner {
        EscrowOrder storage order = escrows[orderId];
        require(order.state == EscrowState.Disputed, "Order not disputed");
        require(buyerShareBps <= 10000, "Invalid share bps");

        uint256 buyerAmount = (order.amount * buyerShareBps) / 10000;
        uint256 sellerTotal = order.amount - buyerAmount;

        uint256 feeAmount = (sellerTotal * order.feeBps) / 10000;
        uint256 sellerAmount = sellerTotal - feeAmount;

        order.state = buyerShareBps == 10000 ? EscrowState.Refunded : EscrowState.Completed;

        if (buyerAmount > 0) {
            _payout(order.token, order.buyer, buyerAmount);
        }
        if (sellerAmount > 0) {
            _payout(order.token, order.seller, sellerAmount);
        }
        if (feeAmount > 0) {
            _payout(order.token, feeRecipient, feeAmount);
        }

        emit EscrowCompleted(orderId, order.seller, sellerAmount, feeAmount);
    }

    /**
     * @notice Anchor a periodic Merkle root of market listings, receipts, or audit log.
     * @param root The 32-byte Merkle root.
     * @param leafCount Number of leaves anchored in this batch.
     */
    function anchorMerkleRoot(bytes32 root, uint256 leafCount) external onlyOwner {
        require(root != bytes32(0), "Invalid merkle root");
        require(leafCount > 0, "Leaf count must be positive");
        require(anchoredRoots[root] == 0, "Root already anchored");

        anchoredRoots[root] = block.timestamp;
        emit MerkleRootAnchored(root, leafCount, block.timestamp);
    }

    function _payout(address token, address to, uint256 amount) internal {
        if (amount == 0) return;
        if (token == address(0)) {
            (bool success, ) = payable(to).call{value: amount}("");
            require(success, "Native transfer failed");
        } else {
            (bool success, bytes memory data) = token.call(
                abi.encodeWithSignature("transfer(address,uint256)", to, amount)
            );
            require(success && (data.len() == 0 || abi.decode(data, (bool))), "Token transfer failed");
        }
    }

    function getEscrow(bytes32 orderId) external view returns (EscrowOrder memory) {
        return escrows[orderId];
    }
}
